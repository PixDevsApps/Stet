//! Replace in Files without a toolkit (M8, ADR-005 amendment).
//!
//! A file is searched as an open document would be: its decoded text normalized to LF
//! (ADR-008), so `\r\n` in an Extended query, `\R` and `$` mean what they mean in the editor.
//! The replacements are then written back into the file's own decoded text, with its line
//! endings: [`LfText`] maps every offset of the LF text to the raw text, and [`raw_edits`]
//! turns the line breaks a replacement inserts into the file's dominant line ending. Nothing
//! outside the matches changes, so each untouched line keeps its own line ending, also in a
//! file with mixed ones.
//!
//! The words the results panel uses for a run and for the files it leaves alone are here too.

use crate::search::report::counted;
use crate::text::{Eol, EolStats};
use std::fmt;
use std::ops::Range;

/// A decoded text normalized to LF, with what it takes to map offsets back to the text as it
/// was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LfText {
    /// The LF-normalized text: CRLF and lone CR became LF.
    pub text: String,
    /// The byte offsets in `text` of the LFs that were CRLF, in order.
    crlf: Vec<usize>,
    pub stats: EolStats,
}

impl LfText {
    pub fn new(raw: &str) -> Self {
        let bytes = raw.as_bytes();
        let mut text = String::with_capacity(raw.len());
        let mut crlf = Vec::new();
        let mut stats = EolStats::default();
        let mut start = 0;
        for index in memchr::memchr2_iter(b'\r', b'\n', bytes) {
            if index < start {
                continue;
            }
            text.push_str(&raw[start..index]);
            if bytes[index] == b'\n' {
                stats.lf += 1;
                start = index + 1;
            } else if bytes.get(index + 1) == Some(&b'\n') {
                stats.crlf += 1;
                crlf.push(text.len());
                start = index + 2;
            } else {
                stats.cr += 1;
                start = index + 1;
            }
            text.push('\n');
        }
        text.push_str(&raw[start..]);
        Self { text, crlf, stats }
    }

    /// The byte offset in the raw text of byte `offset` of [`LfText::text`]: an LF that was a
    /// CRLF maps to its CR, the byte after it to the byte after the LF.
    pub fn raw_offset(&self, offset: usize) -> usize {
        offset + self.crlf.partition_point(|&at| at < offset)
    }

    /// The line ending that line breaks a replacement inserts get: the file's most frequent
    /// one, LF when it has none (what saving the document would write).
    pub fn eol(&self) -> Eol {
        self.stats.dominant().unwrap_or_default()
    }
}

/// A replacement in the raw text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEdit {
    /// Byte range in the raw text.
    pub range: Range<usize>,
    pub insert: String,
}

/// Maps replacements found in `lf.text` (byte ranges, sorted and not overlapping, with
/// LF-normalized inserts) to the raw text, where each line break an insert holds becomes
/// [`LfText::eol`].
pub fn raw_edits<'a>(
    lf: &LfText,
    edits: impl IntoIterator<Item = (Range<usize>, &'a str)>,
) -> Vec<RawEdit> {
    let eol = lf.eol();
    edits
        .into_iter()
        .map(|(range, insert)| RawEdit {
            range: lf.raw_offset(range.start)..lf.raw_offset(range.end),
            insert: if eol == Eol::Lf {
                insert.to_owned()
            } else {
                insert.replace('\n', eol.as_str())
            },
        })
        .collect()
}

/// `raw` with `edits` (sorted, not overlapping) applied, or `None` when that would join two
/// line breaks into one: in a file with mixed line endings, a CR (kept, or inserted as the
/// file's line ending) that ends up right before an LF it wasn't paired with reads back as
/// one CRLF, where the open document would have two lines.
pub fn apply_raw(raw: &str, edits: &[RawEdit]) -> Option<String> {
    let mut out = String::with_capacity(raw.len());
    let mut boundaries = Vec::with_capacity(edits.len() * 2);
    let mut copied = 0;
    for edit in edits {
        out.push_str(&raw[copied..edit.range.start]);
        boundaries.push(out.len());
        out.push_str(&edit.insert);
        boundaries.push(out.len());
        copied = edit.range.end;
    }
    out.push_str(&raw[copied..]);
    let bytes = out.as_bytes();
    let joins = boundaries
        .iter()
        .any(|&at| at > 0 && bytes.get(at - 1) == Some(&b'\r') && bytes.get(at) == Some(&b'\n'));
    (!joins).then_some(out)
}

/// Why Replace in Files left a file alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    /// NUL bytes near the start that are not UTF-16: the editor opens it read-only.
    Binary,
    /// Decoding met malformed bytes, or encoding the text again would not give the bytes back.
    Inexact { encoding: String },
    /// A replacement holds characters the file's encoding cannot represent; the first few.
    Unmappable {
        encoding: String,
        chars: Vec<char>,
        count: usize,
    },
    /// The user may not write the file.
    ReadOnly,
    /// The file changed after it was read, before it was written.
    ChangedMeanwhile,
    /// Larger than the limit, in bytes.
    TooLarge { size: u64, limit: u64 },
    /// The bytes outside the replacements could not be kept exactly (a stateful encoding
    /// such as ISO-2022-JP), so nothing was written.
    BytesNotKept { encoding: String },
    /// Mixed line endings: a replacement would leave a CR next to an LF it wasn't paired
    /// with, which reads back as one line break instead of two ([`apply_raw`]).
    LineBreaksWouldJoin,
    /// An open document that can't be edited: read-only, binary or broken for display.
    DocumentReadOnly,
    /// An open document that changed while its replacements were worked out.
    DocumentChanged,
    /// An open document closed before its turn, so neither it nor its file was changed.
    DocumentClosed,
    /// An open document whose text didn't finish loading.
    DocumentNotLoaded,
}

impl fmt::Display for Skip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Binary => f.write_str("binary file"),
            Self::Inexact { encoding } => write!(
                f,
                "can't be read exactly as {encoding}, so writing it would change other bytes"
            ),
            Self::Unmappable {
                encoding,
                chars,
                count,
            } => {
                let listed: String = chars.iter().collect();
                write!(
                    f,
                    "{encoding} can't represent {} of the replacement ({listed})",
                    counted(*count, "character", "characters")
                )
            }
            Self::ReadOnly => f.write_str("read-only"),
            Self::ChangedMeanwhile => f.write_str("changed on disk while replacing"),
            Self::TooLarge { size, limit } => write!(
                f,
                "{} MiB, over the {} MiB limit; open it to replace in it",
                size.div_ceil(1 << 20),
                limit >> 20
            ),
            Self::BytesNotKept { encoding } => {
                write!(f, "its other bytes couldn't be kept exactly in {encoding}")
            }
            Self::LineBreaksWouldJoin => f.write_str(
                "mixed line endings: replacing would join a CR and an LF into one line break",
            ),
            Self::DocumentReadOnly => f.write_str("the open document is read-only"),
            Self::DocumentChanged => f.write_str("the open document changed while replacing"),
            Self::DocumentClosed => f.write_str("the document was closed while replacing"),
            Self::DocumentNotLoaded => f.write_str("the open document didn't finish loading"),
        }
    }
}

/// What happened to an open document that Replace in Files reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentOutcome {
    /// It had no unsaved changes: the edit was saved, and Undo takes it back in the editor.
    Saved,
    /// It had unsaved changes, so the edit stays in the editor, unsaved.
    Unsaved,
    /// It had no unsaved changes, but saving it as it is would change other bytes (mixed
    /// line endings, a lossy decode, formatted text) or its file changed on disk: unsaved.
    NotSaved,
}

impl DocumentOutcome {
    pub const fn describe(self) -> &'static str {
        match self {
            Self::Saved => "in the open document, saved",
            Self::Unsaved => "in the open document, which has unsaved changes",
            Self::NotSaved => "in the open document, not saved",
        }
    }
}

/// The run's heading in the results panel: `Replace “a” with “b” (3 replacements in 2 files
/// of 10 searched; 1 skipped)`.
pub fn heading(
    pattern: &str,
    replacement: &str,
    replacements: usize,
    files: usize,
    searched: Option<usize>,
    skipped: usize,
) -> String {
    let mut heading = format!(
        "Replace “{}” with “{}” ({} in {}",
        shown(pattern),
        shown(replacement),
        counted(replacements, "replacement", "replacements"),
        counted(files, "file", "files")
    );
    if let Some(searched) = searched {
        heading.push_str(&format!(" of {searched} searched"));
    }
    if skipped > 0 {
        heading.push_str(&format!("; {skipped} skipped"));
    }
    heading.push(')');
    heading
}

/// A file's row: `3 replacements`, with how an open document took them.
pub fn file_summary(replacements: usize, document: Option<DocumentOutcome>) -> String {
    let count = counted(replacements, "replacement", "replacements");
    match document {
        Some(outcome) => format!("{count} {}", outcome.describe()),
        None => count,
    }
}

/// A pattern or replacement on one line.
fn shown(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\n' => '⏎',
            '\r' => '␍',
            '\t' => '⇥',
            c => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::normalize_to_lf;
    use proptest::prelude::*;

    #[test]
    fn crlf_maps_to_both_bytes_and_cr_to_one() {
        let lf = LfText::new("a\r\nb\rc\nd\r\n");
        assert_eq!(lf.text, "a\nb\nc\nd\n");
        assert_eq!((lf.stats.lf, lf.stats.crlf, lf.stats.cr), (1, 2, 1));
        assert_eq!(lf.eol(), Eol::CrLf);
        // "a" stays, the first LF is the CR of "\r\n", "b" moves by one.
        assert_eq!(lf.raw_offset(0), 0);
        assert_eq!(lf.raw_offset(1), 1);
        assert_eq!(lf.raw_offset(2), 3);
        assert_eq!(lf.raw_offset(3), 4);
        assert_eq!(lf.raw_offset(4), 5);
        assert_eq!(lf.raw_offset(7), 8);
        assert_eq!(lf.raw_offset(8), 10);
    }

    #[test]
    fn a_replaced_line_break_takes_both_bytes_and_inserted_ones_the_dominant_ending() {
        let raw = "one\r\ntwo\nthree\r\n";
        let lf = LfText::new(raw);
        // "\n" after "one" (a CRLF) becomes a space; "two" becomes two lines.
        let edits = raw_edits(&lf, [(3..4, " "), (4..7, "2\n2")]);
        assert_eq!(
            apply_raw(raw, &edits).as_deref(),
            Some("one 2\r\n2\nthree\r\n")
        );
        let cr_only = LfText::new("a\rb\r");
        let edits = raw_edits(&cr_only, [(0..1, "x\ny")]);
        assert_eq!(apply_raw("a\rb\r", &edits).as_deref(), Some("x\ry\rb\r"));
        let none = LfText::new("plain");
        let edits = raw_edits(&none, [(0..5, "p\nq")]);
        assert_eq!(apply_raw("plain", &edits).as_deref(), Some("p\nq"));
    }

    #[test]
    fn a_match_that_ends_before_a_crlf_leaves_it() {
        let raw = "key = 1\r\nnext\r\n";
        let lf = LfText::new(raw);
        let edits = raw_edits(&lf, [(6..7, "2")]);
        assert_eq!(
            apply_raw(raw, &edits).as_deref(),
            Some("key = 2\r\nnext\r\n")
        );
        let empty = raw_edits(&lf, [(7..7, ";")]);
        assert_eq!(
            apply_raw(raw, &empty).as_deref(),
            Some("key = 1;\r\nnext\r\n")
        );
    }

    #[test]
    fn line_breaks_that_would_join_refuse_the_edit() {
        // Deleting what stands between a lone CR and a lone LF.
        let raw = "a\rX\nb\r\n";
        let lf = LfText::new(raw);
        assert_eq!(apply_raw(raw, &raw_edits(&lf, [(2..3, "")])), None);
        assert!(apply_raw(raw, &raw_edits(&lf, [(2..3, "Y")])).is_some());
        // A CR-dominant file: an inserted line break before a stray LF.
        let raw = "x\r\ry\n";
        let lf = LfText::new(raw);
        assert_eq!(lf.eol(), Eol::Cr);
        assert_eq!(apply_raw(raw, &raw_edits(&lf, [(3..4, "z\n")])), None);
        // An LF-dominant file: an inserted line break after a stray CR.
        let raw = "a\nb\nc\r";
        let lf = LfText::new(raw);
        assert_eq!(apply_raw(raw, &raw_edits(&lf, [(6..6, "\nX")])), None);
        assert!(apply_raw(raw, &raw_edits(&lf, [(6..6, "X\n")])).is_some());
    }

    #[test]
    fn reports_read_naturally() {
        assert_eq!(
            heading("a\tb", "c\nd", 3, 2, Some(10), 1),
            "Replace “a⇥b” with “c⏎d” (3 replacements in 2 files of 10 searched; 1 skipped)"
        );
        assert_eq!(
            heading("x", "", 1, 1, None, 0),
            "Replace “x” with “” (1 replacement in 1 file)"
        );
        assert_eq!(file_summary(2, None), "2 replacements");
        assert_eq!(
            file_summary(1, Some(DocumentOutcome::Saved)),
            "1 replacement in the open document, saved"
        );
        assert_eq!(
            Skip::Unmappable {
                encoding: "windows-1252".into(),
                chars: vec!['Ω', '✓'],
                count: 3
            }
            .to_string(),
            "windows-1252 can't represent 3 characters of the replacement (Ω✓)"
        );
        assert_eq!(
            Skip::TooLarge {
                size: 60 << 20,
                limit: 50 << 20
            }
            .to_string(),
            "60 MiB, over the 50 MiB limit; open it to replace in it"
        );
    }

    fn raw_text() -> impl Strategy<Value = String> {
        proptest::collection::vec(
            prop_oneof![
                Just("a"),
                Just("bc"),
                Just("é"),
                Just("漢"),
                Just("␀"),
                Just("\n"),
                Just("\r\n"),
                Just("\r"),
                Just(" "),
            ],
            0..40,
        )
        .prop_map(|parts| parts.concat())
    }

    /// Sorted, non-overlapping byte ranges at character boundaries of `text`.
    fn spans(text: &str, cuts: &[usize]) -> Vec<Range<usize>> {
        let bounds: Vec<usize> = text
            .char_indices()
            .map(|(index, _)| index)
            .chain([text.len()])
            .collect();
        let mut picked: Vec<usize> = cuts.iter().map(|cut| bounds[cut % bounds.len()]).collect();
        picked.sort_unstable();
        picked
            .chunks(2)
            .filter(|pair| pair.len() == 2)
            .map(|pair| pair[0]..pair[1])
            .collect()
    }

    proptest! {
        #[test]
        fn lf_text_is_the_loaders_normalization(raw in raw_text()) {
            let lf = LfText::new(&raw);
            let normalized = normalize_to_lf(&raw);
            prop_assert_eq!(lf.text.as_str(), normalized.as_ref());
            prop_assert_eq!(lf.stats, EolStats::detect(raw.as_bytes()));
            prop_assert_eq!(lf.raw_offset(lf.text.len()), raw.len());
        }

        #[test]
        fn raw_edits_change_only_the_matches(
            raw in raw_text(),
            cuts in proptest::collection::vec(any::<usize>(), 0..8),
            inserts in proptest::collection::vec("[xy\n␀]{0,3}", 4),
        ) {
            let lf = LfText::new(&raw);
            let ranges = spans(&lf.text, &cuts);
            let edits: Vec<(Range<usize>, &str)> = ranges
                .iter()
                .cloned()
                .zip(inserts.iter().map(String::as_str).cycle())
                .collect();
            let mapped = raw_edits(&lf, edits.iter().cloned());
            for edit in &mapped {
                prop_assert!(raw.is_char_boundary(edit.range.start));
                prop_assert!(raw.is_char_boundary(edit.range.end));
                // Inserted line breaks take the dominant ending.
                if lf.eol() != Eol::Lf {
                    prop_assert!(!edit.insert.replace(lf.eol().as_str(), "").contains('\n'));
                }
            }
            // A refused edit joins a CR and an LF at one of its edges.
            let Some(replaced) = apply_raw(&raw, &mapped) else {
                return Ok(());
            };
            // Normalizing the result gives the LF text with the edits, as an open document
            // would have it.
            let mut expected = String::new();
            let mut copied = 0;
            for (range, insert) in &edits {
                expected.push_str(&lf.text[copied..range.start]);
                expected.push_str(insert);
                copied = range.end;
            }
            expected.push_str(&lf.text[copied..]);
            let normalized = normalize_to_lf(&replaced);
            prop_assert_eq!(normalized.as_ref(), expected.as_str());
            // Outside the edits the raw text, line endings included, is untouched.
            let mut kept = 0;
            let mut out = 0;
            for edit in &mapped {
                let before = &raw[kept..edit.range.start];
                prop_assert_eq!(&replaced[out..out + before.len()], before);
                out += before.len() + edit.insert.len();
                kept = edit.range.end;
            }
            prop_assert_eq!(&replaced[out..], &raw[kept..]);
        }
    }
}
