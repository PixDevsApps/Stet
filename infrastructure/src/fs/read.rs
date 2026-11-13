//! Reading a file for a tab: decoded, with its longest line and its line count, so the
//! editor knows before inserting it whether its lines are too long to show.

use std::path::Path;
use stet_domain::text::{LongestLine, line_count, longest_line};

use super::document::{
    DecodeReport, EncodingChoice, OpenedDocument, open_document, open_document_as,
};
use super::load::LoadError;
use super::meta::FileMeta;
use super::size::{SizeClass, SizePolicy};

/// What decoding found, without the text, and the file it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocInfo {
    pub report: DecodeReport,
    pub meta: FileMeta,
    pub class: SizeClass,
}

/// A decoded file, ready to insert unless its longest line is over
/// [`stet_domain::text::LONG_LINE_CHARS`].
#[derive(Debug)]
pub struct ReadFile {
    pub info: DocInfo,
    /// LF-normalized, with NUL mapped to the placeholder.
    pub text: String,
    pub longest: LongestLine,
    /// Counted here, since a pass over 200 MB has no place on the GTK thread.
    pub lines: usize,
}

/// Reads and decodes `path` for a tab, as `choice` when the user picked an encoding. Blocks:
/// call it on a worker thread.
pub fn read_file(path: &Path, choice: Option<EncodingChoice>) -> Result<ReadFile, LoadError> {
    let policy = SizePolicy::default();
    let opened = match choice {
        Some(choice) => open_document_as(path, &policy, choice)?,
        None => open_document(path, &policy)?,
    };
    let OpenedDocument {
        text,
        report,
        meta,
        class,
    } = opened;
    let longest = longest_line(&text);
    let lines = line_count(&text);
    Ok(ReadFile {
        info: DocInfo {
            report,
            meta,
            class,
        },
        text,
        longest,
        lines,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use stet_domain::ops::pretty::FormatKind;
    use stet_domain::text::{DISPLAY_BREAK_CHARS, Eol, LONG_LINE_CHARS, break_long_lines};

    #[test]
    fn reads_and_flags_long_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("long.json");
        let long = format!("[{}0]", "1,".repeat(LONG_LINE_CHARS / 2 + 1));
        std::fs::write(&path, &long).unwrap();
        let read = read_file(&path, None).unwrap();
        assert!(read.longest.chars > LONG_LINE_CHARS);
        assert_eq!(read.longest.chars, long.len());
        assert_eq!(FormatKind::sniff(&read.text), Some(FormatKind::Json));
        let broken = break_long_lines(&read.text, DISPLAY_BREAK_CHARS);
        assert_eq!(longest_line(&broken).chars, DISPLAY_BREAK_CHARS);
        assert_eq!(
            line_count(&broken),
            long.len().div_ceil(DISPLAY_BREAK_CHARS)
        );
    }

    #[test]
    fn normalizes_counts_lines_and_reports_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crlf.txt");
        std::fs::write(&path, "one\r\ntwo\r\nthree\r\n").unwrap();
        let read = read_file(&path, None).unwrap();
        assert_eq!(read.text, "one\ntwo\nthree\n");
        assert_eq!(read.lines, 4);
        assert_eq!(read.longest.chars, 5);
        assert_eq!(read.info.meta.size, 17);
        assert_eq!(read.info.class, SizeClass::Normal);
        assert_eq!(read.info.report.eol(), Eol::CrLf);
    }

    #[test]
    fn a_missing_file_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let error = read_file(&dir.path().join("missing.txt"), None).unwrap_err();
        assert!(matches!(error, LoadError::NotFound { .. }), "{error:?}");
    }
}
