//! The Extended search mode: literal text with backslash escapes.
//!
//! The accepted escapes:
//!
//! | Escape | Result |
//! |---|---|
//! | `\n` `\r` `\t` | LF, CR, TAB |
//! | `\0` | NUL |
//! | `\\` | a backslash |
//! | `\bBBBBBBBB` | the character with this 8-digit binary code |
//! | `\oOOO` | 3 octal digits |
//! | `\dDDD` | 3 decimal digits (up to 999) |
//! | `\xHH` | 2 hex digits |
//! | `\uHHHH` | 4 hex digits; a UTF-16 surrogate pair such as `🚂` is one character |
//!
//! Any other backslash sequence stays as typed (`\q` is a backslash and `q`), a numeric escape
//! without enough valid digits stays as typed (`\x4` is `\`, `x`, `4`), and a trailing
//! backslash is literal. The only error is a lone UTF-16 surrogate, which no text can hold.
//!
//! The result is exact: `\r` stays CR and `\0` stays NUL. The translation to buffer text
//! (LF-only, NUL as U+2400) happens where it is searched or inserted
//! ([`super::to_buffer_text`]).

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EscapeError {
    /// `\uHHHH` names half of a UTF-16 surrogate pair without its other half. `offset` is the
    /// character offset of the backslash.
    LoneSurrogate { offset: usize, value: u32 },
}

impl EscapeError {
    pub fn offset(&self) -> usize {
        match self {
            Self::LoneSurrogate { offset, .. } => *offset,
        }
    }
}

impl fmt::Display for EscapeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LoneSurrogate { value, .. } => {
                write!(f, "\\u{value:04X} is half of a surrogate pair")
            }
        }
    }
}

impl std::error::Error for EscapeError {}

/// Replaces Extended-mode escapes in `text` with the characters they stand for.
pub fn unescape(text: &str) -> Result<String, EscapeError> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c != '\\' || i + 1 == chars.len() {
            out.push(c);
            i += 1;
            continue;
        }
        let escape = chars[i + 1];
        let simple = match escape {
            'n' => Some('\n'),
            'r' => Some('\r'),
            't' => Some('\t'),
            '0' => Some('\0'),
            '\\' => Some('\\'),
            _ => None,
        };
        if let Some(character) = simple {
            out.push(character);
            i += 2;
            continue;
        }
        let Some((digits, radix)) = numeric_escape(escape) else {
            out.push('\\');
            out.push(escape);
            i += 2;
            continue;
        };
        let Some(value) = read_number(&chars[i + 2..], digits, radix) else {
            out.push('\\');
            out.push(escape);
            i += 2;
            continue;
        };
        let mut end = i + 2 + digits;
        let character = if (0xD800..0xDC00).contains(&value) && escape == 'u' {
            let low = (chars.get(end) == Some(&'\\') && chars.get(end + 1) == Some(&'u'))
                .then(|| read_number(&chars[end + 2..], 4, 16))
                .flatten()
                .filter(|low| (0xDC00..0xE000).contains(low));
            let Some(low) = low else {
                return Err(EscapeError::LoneSurrogate { offset: i, value });
            };
            end += 6;
            char::from_u32(0x10000 + ((value - 0xD800) << 10) + (low - 0xDC00))
        } else {
            char::from_u32(value)
        };
        let Some(character) = character else {
            return Err(EscapeError::LoneSurrogate { offset: i, value });
        };
        out.push(character);
        i = end;
    }
    Ok(out)
}

/// The digit count and radix of `\b`, `\o`, `\d`, `\x` and `\u`.
fn numeric_escape(escape: char) -> Option<(usize, u32)> {
    match escape {
        'b' => Some((8, 2)),
        'o' => Some((3, 8)),
        'd' => Some((3, 10)),
        'x' => Some((2, 16)),
        'u' => Some((4, 16)),
        _ => None,
    }
}

/// Exactly `digits` characters of `radix`, or nothing.
fn read_number(chars: &[char], digits: usize, radix: u32) -> Option<u32> {
    let slice = chars.get(..digits)?;
    slice
        .iter()
        .try_fold(0u32, |value, c| Some(value * radix + c.to_digit(radix)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn ok(text: &str) -> String {
        unescape(text).unwrap()
    }

    #[test]
    fn simple_escapes() {
        assert_eq!(ok(r"a\r\nb\tc\0d\\e"), "a\r\nb\tc\0d\\e");
    }

    #[test]
    fn numeric_escapes_need_exactly_their_digits() {
        assert_eq!(ok(r"\x41\x6a"), "Aj");
        assert_eq!(ok(r"\d065\o101\b01000001"), "AAA");
        assert_eq!(ok(r"☺"), "☺");
        assert_eq!(ok(r"\xE9"), "é");
        assert_eq!(ok(r"\d999"), "\u{3E7}");
        assert_eq!(ok(r"\x4"), r"\x4");
        assert_eq!(ok(r"\x4g"), r"\x4g");
        assert_eq!(ok(r"\d08a"), r"\d08a");
        assert_eq!(ok(r"\o8"), r"\o8");
        assert_eq!(ok(r"\b0101"), r"\b0101");
        assert_eq!(ok(r"\u12"), r"\u12");
    }

    #[test]
    fn unknown_escapes_and_a_trailing_backslash_stay_literal() {
        assert_eq!(ok(r"\q\a\e\("), r"\q\a\e\(");
        assert_eq!(ok("end\\"), "end\\");
        assert_eq!(ok(r"\\x41"), r"\x41");
    }

    #[test]
    fn surrogate_pairs_join_and_lone_halves_fail() {
        assert_eq!(ok(r"🚂"), "🚂");
        assert_eq!(
            unescape(r"ab\uD83D"),
            Err(EscapeError::LoneSurrogate {
                offset: 2,
                value: 0xD83D
            })
        );
        assert!(unescape(r"\uDE82").is_err());
        assert!(unescape(r"\uD83DA").is_err());
    }

    fn escape(text: &str) -> String {
        text.chars()
            .map(|c| match c {
                '\\' => r"\\".to_owned(),
                '\n' => r"\n".to_owned(),
                '\r' => r"\r".to_owned(),
                '\t' => r"\t".to_owned(),
                '\0' => r"\0".to_owned(),
                c if u32::from(c) > 0xFFFF => {
                    let mut units = [0; 2];
                    c.encode_utf16(&mut units)
                        .iter()
                        .map(|unit| format!(r"\u{unit:04X}"))
                        .collect()
                }
                c if !c.is_ascii() => format!(r"\u{:04X}", u32::from(c)),
                c => c.to_string(),
            })
            .collect()
    }

    proptest! {
        #[test]
        fn escaping_round_trips(text in "[a-z\\\\\\n\\r\\t\\x00éx☺🚂 ]{0,24}") {
            prop_assert_eq!(unescape(&escape(&text)).unwrap(), text);
        }

        #[test]
        fn text_without_backslashes_is_unchanged(text in "[^\\\\]{0,32}") {
            prop_assert_eq!(unescape(&text).unwrap(), text);
        }
    }
}
