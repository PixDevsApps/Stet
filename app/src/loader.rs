//! The worker side of opening a file (M3): read and decode it, look for lines too long to
//! show, format or break them when asked, and stream the text to the GTK thread in pieces of
//! about 4 MiB that the editor inserts one per 16 ms (S1: an idle is starved by GtkTextView's
//! validation; one `set_text` blocks for a second at 100 MB). Nothing here touches GTK.

use std::path::Path;
use stet_domain::ops::{Indent, json, xml};
use stet_domain::text::{
    DISPLAY_BREAK_CHARS, Eol, EolStats, LongestLine, break_long_lines, line_count, longest_line,
};
use stet_infrastructure::encoding::{Detection, Encoding};
use stet_infrastructure::fs::{
    EncodingChoice, FileMeta, LoadError, OpenedDocument, SizeClass, SizePolicy, open_document,
    open_document_as, text_chunks,
};

/// The size of the pieces the editor inserts at a time.
pub const CHUNK_BYTES: usize = 4 * 1024 * 1024;

/// The size of the first piece: about a screen's worth, so it shows at once and GtkTextView
/// validates it in milliseconds; until it has, idle work is starved (S1), and a window shown
/// with it could not even register for accessibility.
pub const FIRST_CHUNK_BYTES: usize = 64 * 1024;

/// What decoding found, without the text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocInfo {
    pub encoding: &'static Encoding,
    pub bom: bool,
    pub eol: Eol,
    pub mixed_eol: bool,
    pub eol_stats: EolStats,
    pub detection: Detection,
    pub had_errors: bool,
    pub lossy_roundtrip: bool,
    pub nul_count: usize,
    pub placeholder_conflict: bool,
    pub binary: bool,
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

/// Reads and decodes `path` for a tab, as `choice` when the user picked an encoding. Call it
/// on a worker thread.
pub fn read(path: &Path, choice: Option<EncodingChoice>) -> Result<ReadFile, LoadError> {
    let policy = SizePolicy::default();
    let opened = match choice {
        Some(choice) => open_document_as(path, &policy, choice)?,
        None => open_document(path, &policy)?,
    };
    let OpenedDocument {
        text,
        encoding,
        bom,
        eol,
        mixed_eol,
        eol_stats,
        detection,
        had_errors,
        lossy_roundtrip,
        nul_count,
        placeholder_conflict,
        binary,
        meta,
        class,
    } = opened;
    let longest = longest_line(&text);
    let lines = line_count(&text);
    Ok(ReadFile {
        info: DocInfo {
            encoding,
            bom,
            eol,
            mixed_eol,
            eol_stats,
            detection,
            had_errors,
            lossy_roundtrip,
            nul_count,
            placeholder_conflict,
            binary,
            meta,
            class,
        },
        text,
        longest,
        lines,
    })
}

/// A language the long-line path can pretty-print.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatKind {
    Json,
    Xml,
}

impl FormatKind {
    /// Guesses from the first character that isn't whitespace.
    pub fn sniff(text: &str) -> Option<Self> {
        match text.trim_start().chars().next()? {
            '{' | '[' => Some(Self::Json),
            '<' => Some(Self::Xml),
            _ => None,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::Xml => "XML",
        }
    }

    /// The GtkSourceView language id.
    pub const fn language(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Xml => "xml",
        }
    }
}

/// Pretty-prints `text` (domain::ops, which keeps every token as written). Errors read as
/// "expected `,` at line 1, column 42".
pub fn format(text: &str, kind: FormatKind) -> Result<String, String> {
    let formatted = match kind {
        FormatKind::Json => json::format(text, Indent::default()),
        FormatKind::Xml => xml::format(text, Indent::default()),
    };
    formatted.map_err(|error| error.to_string())
}

/// Breaks lines longer than [`DISPLAY_BREAK_CHARS`] for display.
pub fn display_breaks(text: &str) -> String {
    break_long_lines(text, DISPLAY_BREAK_CHARS)
}

/// Streams `text` to the receiver in pieces that end at line breaks, the first of about
/// [`FIRST_CHUNK_BYTES`] and the rest of about [`CHUNK_BYTES`], from a thread of its own, so
/// the GTK thread never holds the whole text. The thread stops when the receiver is dropped.
pub fn stream(text: String) -> async_channel::Receiver<String> {
    let (sender, receiver) = async_channel::unbounded();
    let spawned = std::thread::Builder::new()
        .name("stet-loader".into())
        .spawn(move || {
            let first = text_chunks(&text, FIRST_CHUNK_BYTES)
                .next()
                .unwrap_or_default();
            let rest = text_chunks(&text[first.len()..], CHUNK_BYTES);
            for chunk in std::iter::once(first)
                .filter(|first| !first.is_empty())
                .chain(rest)
            {
                if sender.send_blocking(chunk.to_owned()).is_err() {
                    break;
                }
            }
        });
    if let Err(error) = spawned {
        tracing::error!(%error, "could not start the loader thread");
    }
    receiver
}

#[cfg(test)]
mod tests {
    use super::*;
    use stet_domain::text::LONG_LINE_CHARS;

    #[test]
    fn sniffs_json_and_xml() {
        assert_eq!(FormatKind::sniff("  \n{\"a\":1}"), Some(FormatKind::Json));
        assert_eq!(FormatKind::sniff("[1,2]"), Some(FormatKind::Json));
        assert_eq!(
            FormatKind::sniff("<?xml version=\"1.0\"?><a/>"),
            Some(FormatKind::Xml)
        );
        assert_eq!(FormatKind::sniff("plain"), None);
        assert_eq!(FormatKind::sniff(""), None);
    }

    #[test]
    fn formats_or_reports_where_it_failed() {
        assert_eq!(
            format("{\"a\":[1,2]}", FormatKind::Json).unwrap(),
            "{\n  \"a\": [\n    1,\n    2\n  ]\n}"
        );
        let error = format("{\"a\":}", FormatKind::Json).unwrap_err();
        assert!(error.contains("line 1"), "{error}");
        assert!(
            format("<a><b/></a>", FormatKind::Xml)
                .unwrap()
                .contains('\n')
        );
    }

    #[test]
    fn streams_every_byte_in_line_pieces() {
        let text = "line\n".repeat(CHUNK_BYTES / 4);
        let receiver = stream(text.clone());
        let mut joined = String::new();
        let mut pieces = 0;
        while let Ok(piece) = receiver.recv_blocking() {
            assert!(piece.ends_with('\n'));
            let limit = if pieces == 0 {
                FIRST_CHUNK_BYTES
            } else {
                CHUNK_BYTES
            };
            assert!(
                piece.len() <= limit,
                "piece {pieces}: {} bytes",
                piece.len()
            );
            joined.push_str(&piece);
            pieces += 1;
        }
        assert_eq!(joined, text);
        assert_eq!(pieces, 3);
    }

    #[test]
    fn reads_and_flags_long_lines() {
        let dir = std::env::temp_dir().join(format!("stet-loader-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("long.json");
        let long = format!("[{}0]", "1,".repeat(LONG_LINE_CHARS / 2 + 1));
        std::fs::write(&path, &long).unwrap();
        let read = read(&path, None).unwrap();
        assert!(read.longest.chars > LONG_LINE_CHARS);
        assert_eq!(read.longest.chars, long.len());
        assert_eq!(FormatKind::sniff(&read.text), Some(FormatKind::Json));
        let broken = display_breaks(&read.text);
        assert_eq!(longest_line(&broken).chars, DISPLAY_BREAK_CHARS);
        assert_eq!(
            line_count(&broken),
            long.len().div_ceil(DISPLAY_BREAK_CHARS)
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
