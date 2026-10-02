use encoding_rs::{CoderResult, Encoder, EncoderResult, Encoding, UTF_8};
use memchr::memmem;
use stet_domain::text::eol::{EolStats, LfNormalizer};

use super::{PLACEHOLDER, PLACEHOLDER_UTF8, bom_bytes};

/// Input is decoded in chunks of this size, so the only full-size buffer is the text itself.
const CHUNK_LEN: usize = 64 * 1024;

/// Decoded text and what the decode found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    /// The text, with every U+0000 replaced by [`PLACEHOLDER`].
    pub text: String,
    /// Malformed input was replaced with U+FFFD.
    pub had_errors: bool,
    /// How many U+0000 characters became placeholders.
    pub nul_count: usize,
    /// The input already contained U+2400, so placeholders can't all go back to NUL on save.
    pub placeholder_conflict: bool,
    /// Encoding the text again, with [`Decoded::map_placeholder`], does not reproduce the
    /// input after its BOM: decode errors, a non-bijective encoding such as Shift_JIS or
    /// GB18030, or NULs next to real U+2400 characters.
    pub lossy_roundtrip: bool,
}

impl Decoded {
    /// Whether saving maps placeholders back to NUL. Not when the input already had U+2400
    /// characters, which then stay as they were.
    pub fn map_placeholder(&self) -> bool {
        !self.placeholder_conflict
    }
}

/// Decodes `bytes` as `encoding`, stripping the encoding's BOM first when `bom` is set and
/// the input starts with it. Line endings are kept.
pub fn decode(bytes: &[u8], encoding: &'static Encoding, bom: bool) -> Decoded {
    decode_impl(bytes, encoding, bom, None)
}

/// [`decode`] with the line endings normalized to LF in the same pass (ADR-008), and their
/// counts.
pub fn decode_to_lf(bytes: &[u8], encoding: &'static Encoding, bom: bool) -> (Decoded, EolStats) {
    let mut normalizer = LfNormalizer::new();
    let decoded = decode_impl(bytes, encoding, bom, Some(&mut normalizer));
    (decoded, normalizer.finish())
}

fn decode_impl(
    bytes: &[u8],
    encoding: &'static Encoding,
    bom: bool,
    mut normalizer: Option<&mut LfNormalizer>,
) -> Decoded {
    let content = if bom {
        bytes.strip_prefix(bom_bytes(encoding)).unwrap_or(bytes)
    } else {
        bytes
    };
    let mut decoder = encoding.new_decoder_without_bom_handling();
    let mut round_trip = RoundTrip::new(encoding, content);
    let placeholder = memmem::Finder::new(PLACEHOLDER_UTF8);
    let mut text = String::with_capacity(estimated_len(encoding, content.len()));
    let mut piece = String::new();
    let mut mapped = String::new();
    let (mut had_errors, mut nul_count, mut placeholder_conflict) = (false, 0, false);
    let mut rest = content;
    loop {
        let (chunk, tail) = rest.split_at(rest.len().min(CHUNK_LEN));
        rest = tail;
        let last = rest.is_empty();
        piece.clear();
        had_errors |= decode_chunk(&mut decoder, chunk, &mut piece, last);
        if had_errors {
            round_trip = RoundTrip::Unneeded;
        }
        round_trip.feed(&piece, last);
        placeholder_conflict = placeholder_conflict || placeholder.find(piece.as_bytes()).is_some();
        let output = if memchr::memchr(0, piece.as_bytes()).is_some() {
            mapped.clear();
            nul_count += map_nuls(&piece, &mut mapped);
            &mapped
        } else {
            &piece
        };
        match normalizer.as_deref_mut() {
            Some(normalizer) => normalizer.push(output, &mut text),
            None => text.push_str(output),
        }
        if last {
            break;
        }
    }
    if text.capacity() > text.len() + text.len() / 4 + CHUNK_LEN {
        text.shrink_to_fit();
    }
    Decoded {
        text,
        had_errors,
        nul_count,
        placeholder_conflict,
        lossy_roundtrip: had_errors
            || round_trip.failed()
            || (placeholder_conflict && nul_count > 0),
    }
}

/// Decodes one chunk into `piece`, which grows as needed. Returns whether anything was
/// malformed.
fn decode_chunk(
    decoder: &mut encoding_rs::Decoder,
    chunk: &[u8],
    piece: &mut String,
    last: bool,
) -> bool {
    let mut replaced_any = false;
    let mut src = chunk;
    loop {
        let needed = decoder
            .max_utf8_buffer_length(src.len())
            .unwrap_or(src.len());
        piece.reserve(needed);
        let (result, read, replaced) = decoder.decode_to_string(src, piece, last);
        replaced_any |= replaced;
        src = &src[read..];
        if matches!(result, CoderResult::InputEmpty) {
            return replaced_any;
        }
    }
}

/// Copies `piece` into `out` with every NUL replaced by the placeholder, returning the count.
fn map_nuls(piece: &str, out: &mut String) -> usize {
    let mut count = 0;
    let mut start = 0;
    for index in memchr::memchr_iter(0, piece.as_bytes()) {
        out.push_str(&piece[start..index]);
        out.push(PLACEHOLDER);
        start = index + 1;
        count += 1;
    }
    out.push_str(&piece[start..]);
    count
}

/// A first guess at the UTF-8 length, so large files rarely reallocate.
fn estimated_len(encoding: &'static Encoding, input_len: usize) -> usize {
    if encoding == UTF_8 || super::is_utf16(encoding) {
        input_len
    } else if encoding.is_single_byte() {
        input_len + input_len / 16
    } else {
        input_len + input_len / 2
    }
}

/// Re-encodes the decoded text piece by piece and compares it with the input, without
/// holding a second copy of the file.
enum RoundTrip<'a> {
    /// The decode alone decides: UTF-8 and UTF-16 decode without errors only from their own
    /// encoding of the text, and the replacement encoding always reports an error.
    Unneeded,
    Compare {
        encoder: Encoder,
        original: &'a [u8],
        matched: usize,
        buffer: Vec<u8>,
        failed: bool,
    },
}

impl<'a> RoundTrip<'a> {
    fn new(encoding: &'static Encoding, original: &'a [u8]) -> Self {
        if encoding == UTF_8 || encoding.output_encoding() != encoding {
            Self::Unneeded
        } else {
            Self::Compare {
                encoder: encoding.new_encoder(),
                original,
                matched: 0,
                buffer: Vec::with_capacity(CHUNK_LEN),
                failed: false,
            }
        }
    }

    fn feed(&mut self, piece: &str, last: bool) {
        let Self::Compare {
            encoder,
            original,
            matched,
            buffer,
            failed,
        } = self
        else {
            return;
        };
        let mut src = piece;
        while !*failed {
            buffer.clear();
            let (result, read) =
                encoder.encode_from_utf8_to_vec_without_replacement(src, buffer, last);
            let end = *matched + buffer.len();
            if original.get(*matched..end) != Some(buffer.as_slice()) {
                *failed = true;
                return;
            }
            *matched = end;
            src = &src[read..];
            match result {
                EncoderResult::InputEmpty => return,
                EncoderResult::OutputFull => {}
                EncoderResult::Unmappable(_) => *failed = true,
            }
        }
    }

    fn failed(&self) -> bool {
        match self {
            Self::Unneeded => false,
            Self::Compare {
                original,
                matched,
                failed,
                ..
            } => *failed || *matched != original.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encoding_rs::{
        GB18030, ISO_2022_JP, REPLACEMENT, SHIFT_JIS, UTF_16BE, UTF_16LE, WINDOWS_1252,
    };

    #[test]
    fn strips_only_the_encodings_own_bom() {
        assert_eq!(decode(b"\xEF\xBB\xBFabc", UTF_8, true).text, "abc");
        assert_eq!(decode(b"\xEF\xBB\xBFabc", UTF_8, false).text, "\u{feff}abc");
        assert_eq!(decode(b"abc", UTF_8, true).text, "abc");
        assert_eq!(decode(b"\xFF\xFEa\0", UTF_16LE, true).text, "a");
        assert_eq!(
            decode(b"\xFF\xFEa\0", UTF_8, true).text,
            "\u{fffd}\u{fffd}a\u{2400}"
        );
    }

    #[test]
    fn maps_nul_to_the_placeholder() {
        let decoded = decode(b"a\0b\0\0", UTF_8, false);
        assert_eq!(decoded.text, "a\u{2400}b\u{2400}\u{2400}");
        assert_eq!(decoded.nul_count, 3);
        assert!(!decoded.placeholder_conflict && !decoded.lossy_roundtrip);
        assert!(decoded.map_placeholder());
    }

    #[test]
    fn an_existing_placeholder_is_a_conflict() {
        let only_placeholder = decode("a␀b".as_bytes(), UTF_8, false);
        assert!(only_placeholder.placeholder_conflict);
        assert!(!only_placeholder.map_placeholder());
        assert!(!only_placeholder.lossy_roundtrip);

        let both = decode("a\0b␀".as_bytes(), UTF_8, false);
        assert!(both.placeholder_conflict);
        assert!(both.lossy_roundtrip);
    }

    #[test]
    fn malformed_input_is_lossy() {
        let decoded = decode(b"ok \xFF\xFE", UTF_8, false);
        assert!(decoded.had_errors && decoded.lossy_roundtrip);

        let unpaired = decode(&[0x00, 0xD8, 0x61, 0x00], UTF_16LE, false);
        assert_eq!(unpaired.text, "\u{fffd}a");
        assert!(unpaired.had_errors && unpaired.lossy_roundtrip);

        let odd_length = decode(&[0x00, 0x61, 0x00], UTF_16BE, false);
        assert!(odd_length.had_errors && odd_length.lossy_roundtrip);
    }

    #[test]
    fn shift_jis_nec_selected_ibm_extensions_are_lossy() {
        let decoded = decode(b"\x93\xFA\xED\x40", SHIFT_JIS, false);
        assert_eq!(decoded.text, "日纊");
        assert!(!decoded.had_errors);
        assert!(decoded.lossy_roundtrip);
        assert!(!decode(b"\x93\xFA\xFA\x5C", SHIFT_JIS, false).lossy_roundtrip);
    }

    #[test]
    fn gb18030_single_byte_euro_is_lossy() {
        let decoded = decode(b"\x80", GB18030, false);
        assert_eq!(decoded.text, "€");
        assert!(decoded.lossy_roundtrip);
        assert!(!decode(b"\xA2\xE3", GB18030, false).lossy_roundtrip);
    }

    #[test]
    fn windows_1252_round_trips_every_byte() {
        let bytes: Vec<u8> = (0..=255).collect();
        let decoded = decode(&bytes, WINDOWS_1252, false);
        assert!(!decoded.had_errors && !decoded.lossy_roundtrip);
        assert_eq!(decoded.nul_count, 1);
    }

    #[test]
    fn iso_2022_jp_needs_its_final_escape() {
        let (encoded, _, _) = ISO_2022_JP.encode("日本");
        assert!(!decode(&encoded, ISO_2022_JP, false).lossy_roundtrip);
        let unterminated = &encoded[..encoded.len() - 3];
        assert!(decode(unterminated, ISO_2022_JP, false).lossy_roundtrip);
    }

    #[test]
    fn the_replacement_encoding_loses_everything() {
        assert!(decode(b"abc", REPLACEMENT, false).lossy_roundtrip);
        assert!(!decode(b"", REPLACEMENT, false).lossy_roundtrip);
    }

    #[test]
    fn chunk_boundaries_do_not_split_characters_or_line_endings() {
        let mut bytes = vec![b'a'; CHUNK_LEN - 1];
        bytes.extend_from_slice("é\r\n".as_bytes());
        bytes.extend(std::iter::repeat_n(b'b', CHUNK_LEN - 4));
        bytes.extend_from_slice(b"\r\nc");
        let (decoded, stats) = decode_to_lf(&bytes, UTF_8, false);
        assert!(!decoded.had_errors);
        assert_eq!(stats.crlf, 2);
        assert_eq!(stats.total(), 2);
        let expected = String::from_utf8(bytes).unwrap().replace("\r\n", "\n");
        assert_eq!(decoded.text, expected);
    }

    #[test]
    fn decode_to_lf_normalizes_and_counts() {
        let (decoded, stats) = decode_to_lf(b"a\r\nb\rc\nd\0", WINDOWS_1252, false);
        assert_eq!(decoded.text, "a\nb\nc\nd\u{2400}");
        assert_eq!((stats.lf, stats.crlf, stats.cr), (1, 1, 1));
        assert!(!decoded.lossy_roundtrip);
    }
}
