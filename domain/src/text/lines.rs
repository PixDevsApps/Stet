//! Long lines. GtkTextView lays out a line as one paragraph and blocks for minutes on a
//! line of millions of characters (S1: at least 89 s for 10 MB), so such lines are never
//! inserted raw: the loader finds them, and the document opens formatted or with breaks
//! added for display.

/// Lines longer than this many characters take the long-line path (PRD; S1).
pub const LONG_LINE_CHARS: usize = 50_000;

/// How many characters a display break leaves on each piece of a long line.
pub const DISPLAY_BREAK_CHARS: usize = 1_000;

/// The longest line of LF-separated text: its zero-based index and its length in characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LongestLine {
    pub line: usize,
    pub chars: usize,
}

/// The number of lines of LF-separated text, as GtkTextBuffer counts them: one more than
/// its line breaks.
pub fn line_count(text: &str) -> usize {
    memchr::memchr_iter(b'\n', text.as_bytes()).count() + 1
}

/// Finds the longest line, counting characters only for lines whose byte length could beat
/// the longest so far.
pub fn longest_line(text: &str) -> LongestLine {
    let bytes = text.as_bytes();
    let mut longest = LongestLine::default();
    let mut start = 0;
    let ends = memchr::memchr_iter(b'\n', bytes).chain(std::iter::once(bytes.len()));
    for (line, end) in ends.enumerate() {
        if end - start > longest.chars {
            let chars = text[start..end].chars().count();
            if chars > longest.chars {
                longest = LongestLine { line, chars };
            }
        }
        start = end + 1;
    }
    longest
}

/// `text` with a line break after every `width` characters of each line that is longer, so
/// that it can be shown; shorter lines are copied as they are. `width` must not be 0.
pub fn break_long_lines(text: &str, width: usize) -> String {
    assert!(width > 0, "a break width of 0");
    let mut out = String::with_capacity(text.len() + text.len() / width + 1);
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        if line.len() <= width {
            out.push_str(line);
            continue;
        }
        let mut count = 0;
        let mut piece = 0;
        for (offset, _) in line.char_indices() {
            if count == width {
                out.push_str(&line[piece..offset]);
                out.push('\n');
                piece = offset;
                count = 0;
            }
            count += 1;
        }
        out.push_str(&line[piece..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn finds_the_longest_line_in_characters() {
        assert_eq!(longest_line(""), LongestLine { line: 0, chars: 0 });
        assert_eq!(
            longest_line("ab\nabcd\nabc"),
            LongestLine { line: 1, chars: 4 }
        );
        // Three bytes but one character each: the byte length alone would pick line 0.
        assert_eq!(
            longest_line("ééé\nabcd\n"),
            LongestLine { line: 1, chars: 4 }
        );
        assert_eq!(longest_line("a\n\nabc"), LongestLine { line: 2, chars: 3 });
        assert_eq!(
            longest_line("same\nsize"),
            LongestLine { line: 0, chars: 4 }
        );
    }

    #[test]
    fn counts_lines_like_a_text_buffer() {
        assert_eq!(line_count(""), 1);
        assert_eq!(line_count("a"), 1);
        assert_eq!(line_count("a\n"), 2);
        assert_eq!(line_count("a\nb\n\n"), 4);
    }

    #[test]
    fn breaks_only_long_lines() {
        assert_eq!(break_long_lines("abcdefg\nab\n", 3), "abc\ndef\ng\nab\n");
        assert_eq!(break_long_lines("abc\n", 3), "abc\n");
        assert_eq!(break_long_lines("ééééé", 2), "éé\néé\né");
        assert_eq!(break_long_lines("", 4), "");
    }

    proptest! {
        #[test]
        fn breaks_keep_every_character(text in "([a-zé漢]{0,40}\n){0,6}[a-z]{0,40}", width in 1usize..12) {
            let broken = break_long_lines(&text, width);
            prop_assert!(longest_line(&broken).chars <= width);
            prop_assert_eq!(broken.replace('\n', ""), text.replace('\n', ""));
            let unbroken = text.split('\n').filter(|line| line.chars().count() <= width);
            for line in unbroken {
                prop_assert!(broken.split('\n').any(|kept| kept == line));
            }
        }

        #[test]
        fn the_longest_line_matches_a_naive_count(text in "([a-zé漢]{0,20}\n){0,8}[a-zé]{0,20}") {
            let naive = text
                .split('\n')
                .map(|line| line.chars().count())
                .enumerate()
                .fold(LongestLine::default(), |best, (line, chars)| {
                    if chars > best.chars { LongestLine { line, chars } } else { best }
                });
            prop_assert_eq!(longest_line(&text), naive);
        }
    }
}
