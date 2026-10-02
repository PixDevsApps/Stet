use std::fmt;

use encoding_rs::{Encoder, EncoderResult, Encoding, UTF_8, UTF_16LE};
use memchr::memmem;
use stet_domain::text::eol::Eol;

use super::{PLACEHOLDER_UTF8, bom_bytes, is_utf16};

/// How many characters [`Unmappable`] lists; its `count` has the total.
pub const MAX_LISTED_UNMAPPABLE: usize = 100;

/// Output buffer size when encoding only to find unmappable characters.
const CHECK_BUFFER_LEN: usize = 64 * 1024;

/// Characters the target encoding cannot represent. Nothing was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unmappable {
    pub encoding: &'static Encoding,
    /// The first [`MAX_LISTED_UNMAPPABLE`] of them, with their character offsets in the text.
    pub chars: Vec<(usize, char)>,
    /// How many characters cannot be represented, listed or not.
    pub count: usize,
}

impl fmt::Display for Unmappable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let noun = if self.count == 1 {
            "character"
        } else {
            "characters"
        };
        write!(
            f,
            "{} {noun} cannot be represented in {}",
            self.count,
            self.encoding.name()
        )
    }
}

impl std::error::Error for Unmappable {}

/// Encodes `text` strictly: any character `encoding` cannot represent fails the whole
/// encode instead of being replaced. With `map_placeholder`, every [`super::PLACEHOLDER`]
/// is written as NUL. `bom` prefixes the byte order mark of UTF-8 and UTF-16, and is
/// ignored for encodings that have none. UTF-16 is encoded here, since encoding_rs only
/// decodes it.
pub fn encode(
    text: &str,
    encoding: &'static Encoding,
    bom: bool,
    map_placeholder: bool,
) -> Result<Vec<u8>, Unmappable> {
    encode_with_eol(text, encoding, bom, map_placeholder, Eol::Lf)
}

/// [`encode`] for LF-normalized buffer text, writing every LF as `eol` in the same pass
/// (ADR-008). Offsets in [`Unmappable`] count characters of `text`, like `GtkTextIter`.
pub fn encode_with_eol(
    text: &str,
    encoding: &'static Encoding,
    bom: bool,
    map_placeholder: bool,
    eol: Eol,
) -> Result<Vec<u8>, Unmappable> {
    let mut out = Vec::with_capacity(estimated_len(encoding, text.len()));
    if bom {
        out.extend_from_slice(bom_bytes(encoding));
    }
    if encoding == UTF_8 {
        for_each_piece(text, eol, map_placeholder, |piece, _| {
            out.extend_from_slice(piece.as_bytes());
        });
        return Ok(out);
    }
    if is_utf16(encoding) {
        let little_endian = encoding == UTF_16LE;
        for_each_piece(text, eol, map_placeholder, |piece, _| {
            for unit in piece.encode_utf16() {
                out.extend_from_slice(&if little_endian {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                });
            }
        });
        return Ok(out);
    }
    let mut writer = LegacyWriter::new(encoding, text, out, true);
    for_each_piece(text, eol, map_placeholder, |piece, offset| {
        writer.write(piece, offset, false);
    });
    writer.finish()
}

/// Whether `text` can be encoded, without keeping the output: for checking a Convert
/// before the document's encoding changes.
pub fn check_encodable(
    text: &str,
    encoding: &'static Encoding,
    map_placeholder: bool,
) -> Result<(), Unmappable> {
    if encoding == UTF_8 || is_utf16(encoding) {
        return Ok(());
    }
    let mut writer = LegacyWriter::new(encoding, text, Vec::with_capacity(CHECK_BUFFER_LEN), false);
    for_each_piece(text, Eol::Lf, map_placeholder, |piece, offset| {
        writer.write(piece, offset, false);
    });
    writer.finish().map(drop)
}

fn estimated_len(encoding: &'static Encoding, text_len: usize) -> usize {
    if is_utf16(encoding) {
        text_len * 2 + 2
    } else {
        text_len + 3
    }
}

/// Splits `text` around the characters written differently: LF when `eol` isn't LF, and the
/// placeholder when it maps back to NUL. Calls `emit` with each piece of text or replacement
/// and the byte offset in `text` where it starts.
fn for_each_piece(text: &str, eol: Eol, map_placeholder: bool, mut emit: impl FnMut(&str, usize)) {
    let bytes = text.as_bytes();
    let placeholder = memmem::Finder::new(PLACEHOLDER_UTF8);
    let find_lf = |from: usize| {
        (eol != Eol::Lf)
            .then(|| memchr::memchr(b'\n', &bytes[from..]))
            .flatten()
            .map(|index| index + from)
    };
    let find_placeholder = |from: usize| {
        map_placeholder
            .then(|| placeholder.find(&bytes[from..]))
            .flatten()
            .map(|index| index + from)
    };
    let mut start = 0;
    let mut next_lf = find_lf(0);
    let mut next_placeholder = find_placeholder(0);
    loop {
        let (at, is_lf) = match (next_lf, next_placeholder) {
            (Some(lf), Some(placeholder)) if lf < placeholder => (lf, true),
            (Some(lf), None) => (lf, true),
            (_, Some(placeholder)) => (placeholder, false),
            (None, None) => break,
        };
        if at > start {
            emit(&text[start..at], start);
        }
        if is_lf {
            emit(eol.as_str(), at);
            start = at + 1;
            next_lf = find_lf(start);
        } else {
            emit("\0", at);
            start = at + PLACEHOLDER_UTF8.len();
            next_placeholder = find_placeholder(start);
        }
    }
    if start < text.len() {
        emit(&text[start..], start);
    }
}

/// Encodes through encoding_rs, recording unmappable characters and carrying on after
/// them so that every one is found.
struct LegacyWriter<'t> {
    encoder: Encoder,
    out: Vec<u8>,
    keep_output: bool,
    unmappable: Collector<'t>,
}

impl<'t> LegacyWriter<'t> {
    fn new(encoding: &'static Encoding, text: &'t str, out: Vec<u8>, keep_output: bool) -> Self {
        Self {
            encoder: encoding.new_encoder(),
            out,
            keep_output,
            unmappable: Collector::new(encoding, text),
        }
    }

    fn write(&mut self, piece: &str, offset: usize, last: bool) {
        let mut consumed = 0;
        loop {
            let src = &piece[consumed..];
            if self.keep_output {
                let needed = self
                    .encoder
                    .max_buffer_length_from_utf8_without_replacement(src.len())
                    .unwrap_or(src.len());
                self.out.reserve(needed);
            } else {
                self.out.clear();
            }
            let (result, read) =
                self.encoder
                    .encode_from_utf8_to_vec_without_replacement(src, &mut self.out, last);
            consumed += read;
            match result {
                EncoderResult::InputEmpty => return,
                EncoderResult::OutputFull => {}
                EncoderResult::Unmappable(character) => self
                    .unmappable
                    .record(offset + consumed - character.len_utf8(), character),
            }
        }
    }

    fn finish(mut self) -> Result<Vec<u8>, Unmappable> {
        self.write("", 0, true);
        match self.unmappable.into_error() {
            Some(error) => Err(error),
            None => Ok(self.out),
        }
    }
}

struct Collector<'t> {
    text: &'t str,
    error: Unmappable,
    counted_bytes: usize,
    counted_chars: usize,
}

impl<'t> Collector<'t> {
    fn new(encoding: &'static Encoding, text: &'t str) -> Self {
        Self {
            text,
            error: Unmappable {
                encoding,
                chars: Vec::new(),
                count: 0,
            },
            counted_bytes: 0,
            counted_chars: 0,
        }
    }

    fn record(&mut self, byte_offset: usize, character: char) {
        self.error.count += 1;
        if self.error.chars.len() < MAX_LISTED_UNMAPPABLE {
            self.counted_chars += self.text[self.counted_bytes..byte_offset].chars().count();
            self.counted_bytes = byte_offset;
            self.error.chars.push((self.counted_chars, character));
        }
    }

    fn into_error(self) -> Option<Unmappable> {
        (self.error.count > 0).then_some(self.error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::{ENCODINGS, PLACEHOLDER, decode};
    use encoding_rs::{ISO_2022_JP, SHIFT_JIS, UTF_16BE, WINDOWS_1252};
    use proptest::prelude::*;

    #[test]
    fn utf16_is_encoded_by_hand_with_its_bom() {
        let text = "a\u{0}é🎉";
        assert_eq!(
            encode(text, UTF_16LE, true, false).unwrap(),
            b"\xFF\xFEa\0\0\0\xE9\0\x3C\xD8\x89\xDF"
        );
        assert_eq!(
            encode(text, UTF_16BE, true, false).unwrap(),
            b"\xFE\xFF\0a\0\0\0\xE9\xD8\x3C\xDF\x89"
        );
    }

    #[test]
    fn placeholders_map_back_to_nul_only_when_asked() {
        let text = "a␀b";
        assert_eq!(encode(text, UTF_8, false, true).unwrap(), b"a\0b");
        assert_eq!(encode(text, UTF_8, false, false).unwrap(), "a␀b".as_bytes());
        assert_eq!(encode(text, UTF_16BE, false, true).unwrap(), b"\0a\0\0\0b");
        assert_eq!(encode(text, WINDOWS_1252, false, true).unwrap(), b"a\0b");
        let error = encode(text, WINDOWS_1252, false, false).unwrap_err();
        assert_eq!(error.chars, vec![(1, PLACEHOLDER)]);
    }

    #[test]
    fn line_endings_are_written_in_the_same_pass() {
        let text = "a\nb␀\n";
        assert_eq!(
            encode_with_eol(text, UTF_8, true, true, Eol::CrLf).unwrap(),
            b"\xEF\xBB\xBFa\r\nb\0\r\n"
        );
        assert_eq!(
            encode_with_eol(text, UTF_16LE, false, true, Eol::Cr).unwrap(),
            b"a\0\r\0b\0\0\0\r\0"
        );
        assert_eq!(
            encode_with_eol(text, SHIFT_JIS, false, true, Eol::CrLf).unwrap(),
            b"a\r\nb\0\r\n"
        );
    }

    #[test]
    fn unmappable_characters_are_all_reported_with_character_offsets() {
        let error = encode("ab€✓\nc✓", WINDOWS_1252, false, true).unwrap_err();
        assert_eq!(error.encoding, WINDOWS_1252);
        assert_eq!(error.chars, vec![(3, '✓'), (6, '✓')]);
        assert_eq!(error.count, 2);
        assert_eq!(
            error.to_string(),
            "2 characters cannot be represented in windows-1252"
        );
    }

    #[test]
    fn offsets_count_characters_of_the_lf_text_when_converting_eol() {
        let error = encode_with_eol("é\n\nж", WINDOWS_1252, false, true, Eol::CrLf).unwrap_err();
        assert_eq!(error.chars, vec![(3, 'ж')]);
    }

    #[test]
    fn the_list_is_capped_but_the_count_is_not() {
        let text = "Ω".repeat(MAX_LISTED_UNMAPPABLE + 5);
        let error = encode(&text, WINDOWS_1252, false, true).unwrap_err();
        assert_eq!(error.chars.len(), MAX_LISTED_UNMAPPABLE);
        assert_eq!(error.count, MAX_LISTED_UNMAPPABLE + 5);
        assert_eq!(error.chars[99], (99, 'Ω'));
    }

    #[test]
    fn iso_2022_jp_ends_in_ascii() {
        let bytes = encode("日本", ISO_2022_JP, false, true).unwrap();
        assert_eq!(bytes, b"\x1b$BF|K\\\x1b(B");
    }

    #[test]
    fn check_encodable_matches_encode() {
        assert!(check_encodable("déjà vu", WINDOWS_1252, true).is_ok());
        assert!(check_encodable("anything 🎉", UTF_16LE, false).is_ok());
        let long = format!("{}✓", "x".repeat(3 * CHECK_BUFFER_LEN));
        let error = check_encodable(&long, WINDOWS_1252, true).unwrap_err();
        assert_eq!(error.chars, vec![(3 * CHECK_BUFFER_LEN, '✓')]);
    }

    fn unicode_text() -> impl Strategy<Value = String> {
        prop_oneof![
            any::<String>(),
            "[\\x00-\\x7f\\u{80}-\\u{7ff}\\u{2400}\\u{fffd}\\u{10000}-\\u{10ffff}]{0,64}",
        ]
    }

    proptest! {
        #[test]
        fn unicode_round_trips_through_utf8_and_utf16(text in unicode_text(), bom in any::<bool>()) {
            let expected = text.replace('\0', "\u{2400}");
            let conflict = text.contains(PLACEHOLDER);
            let nuls = text.matches('\0').count();
            for encoding in [UTF_8, UTF_16LE, UTF_16BE] {
                let bytes = encode(&text, encoding, bom, false).unwrap();
                let decoded = decode(&bytes, encoding, bom);
                prop_assert_eq!(&decoded.text, &expected);
                prop_assert!(!decoded.had_errors);
                prop_assert_eq!(decoded.nul_count, nuls);
                prop_assert_eq!(decoded.placeholder_conflict, conflict);
                prop_assert_eq!(decoded.lossy_roundtrip, conflict && nuls > 0);
            }
        }

        #[test]
        fn the_lossy_flag_is_exact_for_every_encoding(
            bytes in proptest::collection::vec(any::<u8>(), 0..96),
            index in 0..ENCODINGS.len(),
        ) {
            let entry = &ENCODINGS[index];
            let decoded = decode(&bytes, entry.encoding, false);
            let saved = encode(&decoded.text, entry.encoding, false, decoded.map_placeholder());
            prop_assert_eq!(
                decoded.lossy_roundtrip,
                saved.as_deref() != Ok(bytes.as_slice()),
                "{}", entry.label
            );
        }
    }
}
