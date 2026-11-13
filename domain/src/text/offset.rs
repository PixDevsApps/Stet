//! Offset conversions over UTF-8 text in one forward pass: characters, bytes and lines.

use super::units::{Anchor, ByteOffset, CharOffset, Line};

/// Characters in `bytes`: the bytes that do not continue a UTF-8 sequence.
pub fn count_chars(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .filter(|&&byte| byte.cast_signed() >= -0x40)
        .count()
}

/// The start of the character after the one at byte `at` (one past the end at the end).
pub fn next_boundary(bytes: &[u8], at: usize) -> usize {
    let mut next = at + 1;
    while bytes.get(next).is_some_and(|&byte| byte & 0xC0 == 0x80) {
        next += 1;
    }
    next
}

/// The byte offset of character `target`, counting from `from` (or from the start of the
/// text when `target` lies before it). `None` beyond the end of `text`.
pub fn char_to_byte(text: &str, from: Anchor, target: CharOffset) -> Option<ByteOffset> {
    const BLOCK: usize = 64;
    if target < from.char {
        return char_to_byte(text, Anchor::START, target);
    }
    let mut remaining = target - from.char;
    let bytes = text.as_bytes();
    let mut at = from.byte.get();
    while remaining >= BLOCK && at + BLOCK <= bytes.len() {
        remaining -= count_chars(&bytes[at..at + BLOCK]);
        at += BLOCK;
        while bytes.get(at).is_some_and(|&byte| byte & 0xC0 == 0x80) {
            at += 1;
        }
    }
    while remaining > 0 {
        if at >= bytes.len() {
            return None;
        }
        at = next_boundary(bytes, at);
        remaining -= 1;
    }
    Some(ByteOffset::new(at))
}

/// Converts non-decreasing byte offsets to character offsets.
#[derive(Debug, Clone)]
pub struct CharCursor<'a> {
    bytes: &'a [u8],
    at: Anchor,
}

impl<'a> CharCursor<'a> {
    /// A cursor at the start of `text`.
    pub fn new(text: &'a str) -> Self {
        Self::at(text, Anchor::START)
    }

    /// A cursor at `at`, a point of `text` known in both units.
    pub fn at(text: &'a str, at: Anchor) -> Self {
        Self {
            bytes: text.as_bytes(),
            at,
        }
    }

    /// The character offset of byte `byte`, which must be on a character boundary. Fastest
    /// when calls don't go backwards; a byte before the last one counts again from the start.
    pub fn char_at(&mut self, byte: ByteOffset) -> CharOffset {
        if byte < self.at.byte {
            self.at = Anchor::START;
        }
        self.at.char += count_chars(&self.bytes[self.at.byte.get()..byte.get()]);
        self.at.byte = byte;
        self.at.char
    }
}

/// Converts non-decreasing character offsets to byte offsets.
#[derive(Debug, Clone)]
pub struct ByteCursor<'a> {
    text: &'a str,
    at: Anchor,
}

impl<'a> ByteCursor<'a> {
    /// A cursor at the start of `text`.
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            at: Anchor::START,
        }
    }

    /// The byte offset of character `char`, or the end of the text beyond it. Fastest when
    /// calls don't go backwards.
    pub fn byte_at(&mut self, char: CharOffset) -> ByteOffset {
        match char_to_byte(self.text, self.at, char) {
            Some(byte) => {
                self.at = Anchor::new(byte, char);
                byte
            }
            None => ByteOffset::new(self.text.len()),
        }
    }
}

/// Counts line feeds up to non-decreasing byte offsets.
#[derive(Debug, Clone)]
pub struct LineCursor<'a> {
    bytes: &'a [u8],
    byte: ByteOffset,
    line: Line,
}

impl<'a> LineCursor<'a> {
    /// A cursor at byte `byte` of `text`.
    pub fn at(text: &'a str, byte: ByteOffset) -> Self {
        let bytes = text.as_bytes();
        Self {
            bytes,
            byte,
            line: Line::new(memchr::memchr_iter(b'\n', &bytes[..byte.get()]).count()),
        }
    }

    /// The line of byte `byte`.
    pub fn line_at(&mut self, byte: ByteOffset) -> Line {
        if byte < self.byte {
            self.byte = ByteOffset::ZERO;
            self.line = Line::ZERO;
        }
        self.line += memchr::memchr_iter(b'\n', &self.bytes[self.byte.get()..byte.get()]).count();
        self.byte = byte;
        self.line
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn chars(offset: usize) -> CharOffset {
        CharOffset::new(offset)
    }

    fn bytes(offset: usize) -> ByteOffset {
        ByteOffset::new(offset)
    }

    fn from(byte: usize, char: usize) -> Anchor {
        Anchor::new(bytes(byte), chars(char))
    }

    #[test]
    fn converts_offsets() {
        let text = "aé\n日x";
        let start = Anchor::START;
        assert_eq!(count_chars(text.as_bytes()), 5);
        assert_eq!(char_to_byte(text, start, chars(2)), Some(bytes(3)));
        assert_eq!(char_to_byte(text, from(3, 2), chars(4)), Some(bytes(7)));
        assert_eq!(char_to_byte(text, start, chars(5)), Some(bytes(8)));
        assert_eq!(char_to_byte(text, start, chars(6)), None);
        assert_eq!(char_to_byte(text, from(7, 4), chars(1)), Some(bytes(1)));
        let mut char_cursor = CharCursor::new(text);
        assert_eq!(char_cursor.char_at(bytes(3)), chars(2));
        assert_eq!(char_cursor.char_at(bytes(7)), chars(4));
        assert_eq!(char_cursor.char_at(bytes(1)), chars(1));
        let mut byte_cursor = ByteCursor::new(text);
        assert_eq!(byte_cursor.byte_at(chars(4)), bytes(7));
        assert_eq!(byte_cursor.byte_at(chars(2)), bytes(3));
        assert_eq!(byte_cursor.byte_at(chars(9)), bytes(8));
        let long = "é".repeat(100) + "x" + &"a".repeat(100);
        assert_eq!(char_to_byte(&long, start, chars(100)), Some(bytes(200)));
        assert_eq!(char_to_byte(&long, start, chars(170)), Some(bytes(270)));
        assert_eq!(char_to_byte(&long, start, chars(201)), Some(bytes(301)));
        assert_eq!(char_to_byte(&long, start, chars(202)), None);
        let mut lines = LineCursor::at(text, bytes(4));
        assert_eq!(lines.line_at(bytes(7)), Line::new(1));
        assert_eq!(lines.line_at(bytes(0)), Line::ZERO);
        assert_eq!(next_boundary(text.as_bytes(), 1), 3);
    }

    proptest! {
        #[test]
        fn cursors_agree_with_char_indices(text in "[a\\né日\\u{1F600}]{0,300}", picks in prop::collection::vec(0usize..320, 0..12)) {
            let starts: Vec<usize> = text.char_indices().map(|(byte, _)| byte).chain([text.len()]).collect();
            let mut picks = picks;
            picks.sort_unstable();
            let mut byte_cursor = ByteCursor::new(&text);
            let mut char_cursor = CharCursor::new(&text);
            for pick in picks {
                let expected = starts.get(pick).copied().unwrap_or(text.len());
                prop_assert_eq!(byte_cursor.byte_at(chars(pick)), bytes(expected));
                if let Some(&byte) = starts.get(pick) {
                    prop_assert_eq!(char_cursor.char_at(bytes(byte)), chars(pick));
                    prop_assert_eq!(char_to_byte(&text, Anchor::START, chars(pick)), Some(bytes(byte)));
                }
            }
        }

        #[test]
        fn any_anchor_gives_the_same_byte(text in "[a\\né日]{0,200}", anchor in 0usize..210, target in 0usize..210) {
            let starts: Vec<usize> = text.char_indices().map(|(byte, _)| byte).chain([text.len()]).collect();
            let anchor = anchor.min(starts.len() - 1);
            let from = Anchor::new(bytes(starts[anchor]), chars(anchor));
            prop_assert_eq!(
                char_to_byte(&text, from, chars(target)),
                char_to_byte(&text, Anchor::START, chars(target))
            );
            prop_assert_eq!(
                char_to_byte(&text, from, chars(target)),
                starts.get(target).map(|&byte| bytes(byte))
            );
        }
    }
}
