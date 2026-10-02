//! Offset conversions over UTF-8 text in one forward pass.

/// Characters in `bytes`: the bytes that do not continue a UTF-8 sequence.
pub(crate) fn count_chars(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&byte| (byte as i8) >= -0x40).count()
}

/// The start of the character after the one at byte `at` (one past the end at the end).
pub(crate) fn next_boundary(bytes: &[u8], at: usize) -> usize {
    let mut next = at + 1;
    while bytes.get(next).is_some_and(|&byte| byte & 0xC0 == 0x80) {
        next += 1;
    }
    next
}

/// The byte offset of character `target`, walking from byte `byte`, which is character
/// `chars`. `None` beyond the end of `text`.
pub(crate) fn char_to_byte(text: &str, byte: usize, chars: usize, target: usize) -> Option<usize> {
    if target < chars {
        return char_to_byte(text, 0, 0, target);
    }
    const BLOCK: usize = 64;
    let mut remaining = target - chars;
    let bytes = text.as_bytes();
    let mut at = byte;
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
    Some(at)
}

/// Converts non-decreasing byte offsets to character offsets.
pub(crate) struct CharCursor<'a> {
    bytes: &'a [u8],
    byte: usize,
    chars: usize,
}

impl<'a> CharCursor<'a> {
    /// A cursor at byte `byte`, which is character `chars`.
    pub(crate) fn at(text: &'a str, byte: usize, chars: usize) -> Self {
        Self {
            bytes: text.as_bytes(),
            byte,
            chars,
        }
    }

    pub(crate) fn char_at(&mut self, byte: usize) -> usize {
        if byte < self.byte {
            self.byte = 0;
            self.chars = 0;
        }
        self.chars += count_chars(&self.bytes[self.byte..byte]);
        self.byte = byte;
        self.chars
    }
}

/// Converts non-decreasing character offsets to byte offsets.
pub(crate) struct ByteCursor<'a> {
    text: &'a str,
    byte: usize,
    chars: usize,
}

impl<'a> ByteCursor<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        Self {
            text,
            byte: 0,
            chars: 0,
        }
    }

    /// The byte offset of character `chars`, or the end of the text beyond it.
    pub(crate) fn byte_at(&mut self, chars: usize) -> usize {
        match char_to_byte(self.text, self.byte, self.chars, chars) {
            Some(byte) => {
                self.byte = byte;
                self.chars = chars;
                byte
            }
            None => self.text.len(),
        }
    }
}

/// Counts line feeds up to non-decreasing byte offsets.
pub(crate) struct LineCursor<'a> {
    bytes: &'a [u8],
    byte: usize,
    line: usize,
}

impl<'a> LineCursor<'a> {
    /// A cursor at byte `byte` of `text`.
    pub(crate) fn at(text: &'a str, byte: usize) -> Self {
        let bytes = text.as_bytes();
        Self {
            bytes,
            byte,
            line: memchr::memchr_iter(b'\n', &bytes[..byte]).count(),
        }
    }

    /// The zero-based line of byte `byte`.
    pub(crate) fn line_at(&mut self, byte: usize) -> usize {
        if byte < self.byte {
            self.byte = 0;
            self.line = 0;
        }
        self.line += memchr::memchr_iter(b'\n', &self.bytes[self.byte..byte]).count();
        self.byte = byte;
        self.line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_offsets() {
        let text = "aé\n日x";
        assert_eq!(count_chars(text.as_bytes()), 5);
        assert_eq!(char_to_byte(text, 0, 0, 2), Some(3));
        assert_eq!(char_to_byte(text, 3, 2, 4), Some(7));
        assert_eq!(char_to_byte(text, 0, 0, 5), Some(8));
        assert_eq!(char_to_byte(text, 0, 0, 6), None);
        assert_eq!(char_to_byte(text, 7, 4, 1), Some(1));
        let mut chars = CharCursor::at(text, 0, 0);
        assert_eq!(chars.char_at(3), 2);
        assert_eq!(chars.char_at(7), 4);
        assert_eq!(chars.char_at(1), 1);
        let mut bytes = ByteCursor::new(text);
        assert_eq!(bytes.byte_at(4), 7);
        assert_eq!(bytes.byte_at(2), 3);
        assert_eq!(bytes.byte_at(9), 8);
        let long = "é".repeat(100) + "x" + &"a".repeat(100);
        assert_eq!(char_to_byte(&long, 0, 0, 100), Some(200));
        assert_eq!(char_to_byte(&long, 0, 0, 170), Some(270));
        assert_eq!(char_to_byte(&long, 0, 0, 201), Some(301));
        assert_eq!(char_to_byte(&long, 0, 0, 202), None);
        let mut lines = LineCursor::at(text, 4);
        assert_eq!(lines.line_at(7), 1);
        assert_eq!(lines.line_at(0), 0);
        assert_eq!(next_boundary(text.as_bytes(), 1), 3);
    }
}
