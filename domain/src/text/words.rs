//! Words as the status bar counts them.

/// Words: runs of characters between white space.
#[must_use]
pub fn count_words(text: &str) -> usize {
    text.split_whitespace().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_runs_between_white_space() {
        assert_eq!(count_words(""), 0);
        assert_eq!(count_words(" \t\n "), 0);
        assert_eq!(count_words("one"), 1);
        assert_eq!(count_words("  one two\tthree\nfour  "), 4);
        assert_eq!(count_words("don't stop-here, ok?"), 3);
        assert_eq!(count_words("日本語 café\u{00A0}crème"), 3);
        assert_eq!(count_words("a\r\nb"), 2);
    }
}
