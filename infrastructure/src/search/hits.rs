use std::ops::Range;

use super::matcher::Match;
use super::text::{ByteCursor, count_chars};

/// The longest line text a [`Hit`] carries.
pub const MAX_LINE_CHARS: usize = 500;
/// How much of a long line is kept before its first match.
const CONTEXT_CHARS: usize = 40;

/// A result line, shared by Find All (in one or all open documents) and Find in Files, so one
/// results panel shows both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// One-based.
    pub line_number: u64,
    /// The line without its line break, decoded as UTF-8 with U+FFFD for invalid bytes. A line
    /// longer than [`MAX_LINE_CHARS`] is cut to a window that starts a little before its first
    /// match.
    pub line_text: String,
    /// Characters of the line before `line_text`: 0 unless the line was cut.
    pub line_offset: usize,
    /// The matches on the line, as byte ranges in `line_text`. A match that goes on past the
    /// line ends at the end of `line_text`.
    pub match_ranges: Vec<Range<usize>>,
}

impl Hit {
    /// The character column (in the whole line) where the first match starts.
    pub fn column(&self) -> usize {
        let start = self.match_ranges.first().map_or(0, |range| range.start);
        self.line_offset + count_chars(&self.line_text.as_bytes()[..start])
    }
}

/// Groups in-document matches (from [`super::Matcher::find_all`] on `text`) into result lines.
pub fn document_hits(text: &str, matches: &[Match]) -> Vec<Hit> {
    let bytes = text.as_bytes();
    let mut cursor = ByteCursor::new(text);
    let mut hits = Vec::new();
    let mut i = 0;
    while i < matches.len() {
        let line = matches[i].line;
        let first = cursor.byte_at(matches[i].start);
        let line_start = memchr::memrchr(b'\n', &bytes[..first]).map_or(0, |at| at + 1);
        let line_end = memchr::memchr(b'\n', &bytes[first..]).map_or(bytes.len(), |at| first + at);
        let mut ranges = Vec::new();
        while let Some(found) = matches.get(i).filter(|found| found.line == line) {
            let start = cursor.byte_at(found.start);
            let end = cursor.byte_at(found.end);
            ranges.push(start - line_start..end.max(start) - line_start);
            i += 1;
        }
        hits.push(make_hit(
            line as u64 + 1,
            &bytes[line_start..line_end],
            &ranges,
        ));
    }
    hits
}

/// A hit from the raw bytes of a line (its line break may be included) and the byte ranges
/// of its matches.
pub(crate) fn make_hit(line_number: u64, line: &[u8], ranges: &[Range<usize>]) -> Hit {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let clip = |range: &Range<usize>| {
        let start = range.start.min(line.len());
        start..range.end.clamp(start, line.len())
    };
    let (text, ranges): (String, Vec<Range<usize>>) = match std::str::from_utf8(line) {
        Ok(text) => (text.to_owned(), ranges.iter().map(clip).collect()),
        Err(_) => decode_lossy(line, &ranges.iter().map(clip).collect::<Vec<_>>()),
    };
    window(line_number, text, ranges)
}

/// Lossy UTF-8 decoding that moves byte ranges along with the text.
fn decode_lossy(line: &[u8], ranges: &[Range<usize>]) -> (String, Vec<Range<usize>>) {
    let mut text = String::with_capacity(line.len() + 8);
    let mut segments: Vec<(usize, usize, bool)> = Vec::new();
    let mut raw = 0;
    for chunk in line.utf8_chunks() {
        segments.push((raw, text.len(), true));
        text.push_str(chunk.valid());
        raw += chunk.valid().len();
        if !chunk.invalid().is_empty() {
            segments.push((raw, text.len(), false));
            text.push(char::REPLACEMENT_CHARACTER);
            raw += chunk.invalid().len();
        }
    }
    let map = |offset: usize| {
        let index = segments.partition_point(|&(start, _, _)| start <= offset);
        match index.checked_sub(1).map(|i| segments[i]) {
            Some((start, decoded, true)) => decoded + (offset - start),
            Some((start, decoded, false)) if offset > start => {
                decoded + char::REPLACEMENT_CHARACTER.len_utf8()
            }
            Some((_, decoded, false)) => decoded,
            None => 0,
        }
    };
    let ranges = ranges
        .iter()
        .map(|range| map(range.start).min(text.len())..map(range.end).min(text.len()))
        .collect();
    (text, ranges)
}

/// Cuts a long line to [`MAX_LINE_CHARS`] around its first match.
fn window(line_number: u64, text: String, ranges: Vec<Range<usize>>) -> Hit {
    if count_chars(text.as_bytes()) <= MAX_LINE_CHARS {
        return Hit {
            line_number,
            line_text: text,
            line_offset: 0,
            match_ranges: ranges,
        };
    }
    let first = ranges.first().map_or(0, |range| range.start);
    let first_char = count_chars(&text.as_bytes()[..first]);
    let start_char = first_char.saturating_sub(CONTEXT_CHARS);
    let mut boundaries = text.char_indices().map(|(at, _)| at).skip(start_char);
    let start = boundaries.next().unwrap_or(text.len());
    let end = boundaries.nth(MAX_LINE_CHARS - 1).unwrap_or(text.len());
    let match_ranges = ranges
        .iter()
        .filter(|range| range.end >= start && range.start <= end)
        .map(|range| range.start.clamp(start, end) - start..range.end.clamp(start, end) - start)
        .collect();
    Hit {
        line_number,
        line_text: text[start..end].to_owned(),
        line_offset: start_char,
        match_ranges,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_matches_by_line() {
        let text = "one two\nthree two two\n";
        let matches = [
            Match {
                start: 4,
                end: 7,
                line: 0,
            },
            Match {
                start: 14,
                end: 17,
                line: 1,
            },
            Match {
                start: 18,
                end: 21,
                line: 1,
            },
        ];
        let hits = document_hits(text, &matches);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].line_number, 1);
        assert_eq!(hits[0].line_text, "one two");
        assert_eq!(
            hits[0].match_ranges.as_slice(),
            std::slice::from_ref(&(4..7))
        );
        assert_eq!(hits[1].line_text, "three two two");
        assert_eq!(hits[1].match_ranges, [6..9, 10..13]);
        assert_eq!(hits[1].column(), 6);
    }

    #[test]
    fn decodes_invalid_bytes_and_moves_ranges() {
        let hit = make_hit(3, b"a\xffb\xe9\xffcd\r\n", std::slice::from_ref(&(5..7)));
        assert_eq!(hit.line_text, "a\u{FFFD}b\u{FFFD}\u{FFFD}cd");
        assert_eq!(&hit.line_text[hit.match_ranges[0].clone()], "cd");
    }

    #[test]
    fn cuts_long_lines_around_the_first_match() {
        let line = format!("{}needle{}", "x".repeat(1000), "y".repeat(1000));
        let hit = make_hit(1, line.as_bytes(), std::slice::from_ref(&(1000..1006)));
        assert_eq!(hit.line_text.chars().count(), MAX_LINE_CHARS);
        assert_eq!(hit.line_offset, 960);
        assert_eq!(&hit.line_text[hit.match_ranges[0].clone()], "needle");
        assert_eq!(hit.column(), 1000);
    }
}
