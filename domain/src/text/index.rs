//! Line starts of a text, for converting between byte offsets, character offsets and
//! line/column positions. Lines end after `\n`; buffer text is LF-normalized (ADR-008).

use std::ops::Range;

/// A zero-based line and a column counted in characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

impl Position {
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineIndex {
    byte_starts: Vec<usize>,
    char_starts: Vec<usize>,
    len_bytes: usize,
    len_chars: usize,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut byte_starts = vec![0];
        let mut char_starts = vec![0];
        let mut chars = 0;
        let mut last = 0;
        for newline in memchr::memchr_iter(b'\n', text.as_bytes()) {
            chars += text[last..=newline].chars().count();
            last = newline + 1;
            byte_starts.push(last);
            char_starts.push(chars);
        }
        chars += text[last..].chars().count();
        Self {
            byte_starts,
            char_starts,
            len_bytes: text.len(),
            len_chars: chars,
        }
    }

    /// The number of lines; a text ending in `\n` has an empty last line.
    pub fn line_count(&self) -> usize {
        self.byte_starts.len()
    }

    pub fn len_chars(&self) -> usize {
        self.len_chars
    }

    pub fn len_bytes(&self) -> usize {
        self.len_bytes
    }

    pub fn line_of_char(&self, offset: usize) -> usize {
        self.char_starts.partition_point(|&start| start <= offset) - 1
    }

    pub fn line_of_byte(&self, offset: usize) -> usize {
        self.byte_starts.partition_point(|&start| start <= offset) - 1
    }

    /// Characters of `line`, without its `\n`.
    pub fn line_chars(&self, line: usize) -> Range<usize> {
        let start = self.char_starts[line];
        let end = self
            .char_starts
            .get(line + 1)
            .map_or(self.len_chars, |next| next - 1);
        start..end
    }

    /// Bytes of `line`, without its `\n`.
    pub fn line_bytes(&self, line: usize) -> Range<usize> {
        let start = self.byte_starts[line];
        let end = self
            .byte_starts
            .get(line + 1)
            .map_or(self.len_bytes, |next| next - 1);
        start..end
    }

    pub fn byte_to_char(&self, text: &str, offset: usize) -> usize {
        let line = self.line_of_byte(offset);
        self.char_starts[line] + text[self.byte_starts[line]..offset].chars().count()
    }

    pub fn char_to_byte(&self, text: &str, offset: usize) -> usize {
        let line = self.line_of_char(offset);
        let start = self.byte_starts[line];
        let column = offset - self.char_starts[line];
        text[start..]
            .char_indices()
            .nth(column)
            .map_or(text.len(), |(byte, _)| start + byte)
    }

    pub fn position(&self, offset: usize) -> Position {
        let line = self.line_of_char(offset);
        Position::new(line, offset - self.char_starts[line])
    }

    /// The character offset of `position`, clamped to the end of its line and of the text.
    pub fn offset(&self, position: Position) -> usize {
        let line = position.line.min(self.line_count() - 1);
        let range = self.line_chars(line);
        (range.start + position.column).min(range.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn indexes_lines_in_bytes_and_characters() {
        let text = "fn main() {\n    let ø = 1;\n}\n";
        let index = LineIndex::new(text);
        assert_eq!(index.line_count(), 4);
        assert_eq!(index.line_chars(1), 12..26);
        assert_eq!(&text[index.line_bytes(1)], "    let ø = 1;");
        assert_eq!(index.line_chars(3), 29..29);
        assert_eq!(index.position(20), Position::new(1, 8));
        assert_eq!(index.offset(Position::new(1, 99)), 26);
        assert_eq!(index.offset(Position::new(9, 0)), 29);
    }

    #[test]
    fn empty_text_has_one_empty_line() {
        let index = LineIndex::new("");
        assert_eq!(index.line_count(), 1);
        assert_eq!(index.line_chars(0), 0..0);
        assert_eq!(index.position(0), Position::default());
    }

    proptest! {
        #[test]
        fn conversions_agree_with_a_naive_model(text in "[a-zø日\n]{0,40}", pick in 0usize..64) {
            let index = LineIndex::new(&text);
            let chars: Vec<(usize, char)> = text.char_indices().collect();
            prop_assert_eq!(index.len_chars(), chars.len());
            let offset = pick % (chars.len() + 1);
            let byte = chars.get(offset).map_or(text.len(), |(b, _)| *b);
            prop_assert_eq!(index.char_to_byte(&text, offset), byte);
            prop_assert_eq!(index.byte_to_char(&text, byte), offset);
            let before = &chars[..offset];
            let line = before.iter().filter(|(_, c)| *c == '\n').count();
            let column = before.iter().rev().take_while(|(_, c)| *c != '\n').count();
            prop_assert_eq!(index.position(offset), Position::new(line, column));
            prop_assert_eq!(index.offset(Position::new(line, column)), offset);
            prop_assert_eq!(index.line_count(), text.matches('\n').count() + 1);
        }
    }
}
