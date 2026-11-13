//! Character rules shared by search, comparison, marking and the encodings.

/// What the buffer holds instead of U+0000, which GtkTextBuffer rejects (ADR-007).
pub const NUL_PLACEHOLDER: char = '\u{2400}';

/// A word character, as Scintilla counts them: a letter, digit or `_`.
#[must_use]
pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `c` lowercased to one character, so that folded text keeps its character count.
#[must_use]
pub fn fold_char(c: char) -> char {
    if c.is_ascii() {
        c.to_ascii_lowercase()
    } else {
        c.to_lowercase().next().unwrap_or(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn word_characters() {
        assert!(['a', 'Z', '7', '_', 'é', 'ж'].into_iter().all(is_word_char));
        assert!(!['.', ' ', '-', '\u{2400}'].into_iter().any(is_word_char));
    }

    #[test]
    fn folding_keeps_one_character() {
        assert_eq!(fold_char('A'), 'a');
        assert_eq!(fold_char('Ä'), 'ä');
        assert_eq!(fold_char('İ'), 'i');
        assert_eq!(fold_char('7'), '7');
    }
}
