use std::cmp::Ordering;

use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{Encoding, GB18030, GBK, ISO_2022_JP, UTF_8, UTF_16BE, UTF_16LE, WINDOWS_1252};

/// How much of a file the NUL sniffing looks at, as in git and grep.
const SNIFF_LEN: usize = 8 * 1024;
/// How much chardetng reads, counted from the first non-ASCII byte.
const CHARDET_LEN: usize = 1024 * 1024;
/// How much of an all-ASCII file with escape bytes the ISO-2022-JP check reads.
const ISO_2022_JP_LEN: usize = 64 * 1024;

/// Which rule decided the encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetectionSource {
    Bom,
    /// The whole file is valid UTF-8, which includes plain ASCII.
    ValidUtf8,
    /// UTF-16 without a BOM, recognized by where its zero bytes sit.
    Utf16Heuristic,
    /// chardetng's guess among the legacy encodings, including ISO-2022-JP for 7-bit files.
    Chardetng,
    /// Binary data that isn't UTF-8 opens as Windows-1252, which maps every byte, so it
    /// round-trips.
    Default,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Detection {
    pub encoding: &'static Encoding,
    /// The file starts with a byte order mark for `encoding`.
    pub bom: bool,
    pub source: DetectionSource,
    /// NUL bytes near the start that don't fit a UTF-16 pattern; the file opens read-only.
    pub binary: bool,
}

/// Detects the encoding of a whole file: BOM, then valid UTF-8, then UTF-16 by its zero
/// bytes, then chardetng. Binary data falls back to Windows-1252. The caller applies any
/// per-file override.
pub fn detect(bytes: &[u8]) -> Detection {
    if let Some(encoding) = bom_encoding(bytes) {
        return Detection {
            encoding,
            bom: true,
            source: DetectionSource::Bom,
            binary: false,
        };
    }
    let sample = &bytes[..bytes.len().min(SNIFF_LEN)];
    let has_nul = memchr::memchr(0, sample).is_some();
    if has_nul && let Some(encoding) = utf16_without_bom(sample) {
        return Detection {
            encoding,
            bom: false,
            source: DetectionSource::Utf16Heuristic,
            binary: false,
        };
    }
    let (encoding, source) = if Encoding::utf8_valid_up_to(bytes) == bytes.len() {
        if !has_nul && is_iso_2022_jp(bytes) {
            (ISO_2022_JP, DetectionSource::Chardetng)
        } else {
            (UTF_8, DetectionSource::ValidUtf8)
        }
    } else if has_nul {
        (WINDOWS_1252, DetectionSource::Default)
    } else {
        (guess_legacy(bytes), DetectionSource::Chardetng)
    };
    Detection {
        encoding,
        bom: false,
        source,
        binary: has_nul,
    }
}

/// The encoding of a leading byte order mark. `FF FE 00 00` is the UTF-32LE mark, which
/// Stet doesn't support, so it isn't read as UTF-16LE followed by U+0000.
fn bom_encoding(bytes: &[u8]) -> Option<&'static Encoding> {
    if bytes.starts_with(&[0xFF, 0xFE, 0, 0]) {
        return None;
    }
    Encoding::for_bom(bytes).map(|(encoding, _)| encoding)
}

/// UTF-16 text has its zero bytes on one side of the code units (the high byte of ASCII and
/// Latin-1 characters), ASCII whitespace forms whole units with a zero byte, and it decodes
/// without errors to mostly printable characters.
fn utf16_without_bom(sample: &[u8]) -> Option<&'static Encoding> {
    let (mut even, mut odd) = (0usize, 0usize);
    for [first, second] in sample.as_chunks::<2>().0 {
        even += usize::from(*first == 0);
        odd += usize::from(*second == 0);
    }
    let (encoding, zeros, stray) = match odd.cmp(&even) {
        Ordering::Greater => (UTF_16LE, odd, even),
        Ordering::Less => (UTF_16BE, even, odd),
        Ordering::Equal => return None,
    };
    if stray * 2 >= zeros {
        return None;
    }
    let (paired, unpaired) = whitespace_units(sample, encoding == UTF_16LE);
    if paired == 0 || paired < unpaired * 2 {
        return None;
    }
    plausible_utf16(sample, encoding).then_some(encoding)
}

/// Counts ASCII whitespace bytes that are the low byte of a code unit whose high byte is
/// zero (`paired`), and those that aren't.
fn whitespace_units(sample: &[u8], little_endian: bool) -> (usize, usize) {
    let (mut paired, mut unpaired) = (0, 0);
    for (index, &byte) in sample.iter().enumerate() {
        if !matches!(byte, b' ' | b'\t' | b'\n' | b'\r') {
            continue;
        }
        let high = if little_endian {
            (index % 2 == 0).then(|| sample.get(index + 1)).flatten()
        } else {
            (index % 2 == 1).then(|| sample.get(index - 1)).flatten()
        };
        if high == Some(&0) {
            paired += 1;
        } else {
            unpaired += 1;
        }
    }
    (paired, unpaired)
}

fn plausible_utf16(sample: &[u8], encoding: &'static Encoding) -> bool {
    let mut decoder = encoding.new_decoder_without_bom_handling();
    let capacity = decoder
        .max_utf8_buffer_length(sample.len())
        .unwrap_or(sample.len() * 3);
    let mut text = String::with_capacity(capacity);
    let (_, _, replaced) = decoder.decode_to_string(sample, &mut text, false);
    if replaced {
        return false;
    }
    let (mut characters, mut suspicious) = (0usize, 0usize);
    for character in text.chars() {
        characters += 1;
        suspicious += usize::from(is_suspicious(character));
    }
    suspicious <= 1 + characters / 20
}

/// Characters that are rare in text but common when binary data is read as UTF-16: C0
/// controls other than whitespace and escape, DEL, C1 controls, private use and
/// noncharacters.
fn is_suspicious(character: char) -> bool {
    matches!(
        character,
        '\0'..='\u{8}'
            | '\u{e}'..='\u{1a}'
            | '\u{1c}'..='\u{1f}'
            | '\u{7f}'..='\u{9f}'
            | '\u{e000}'..='\u{f8ff}'
            | '\u{fdd0}'..='\u{fdef}'
            | '\u{fffe}'
            | '\u{ffff}'
    )
}

/// ISO-2022-JP is 7-bit, so it passes as valid UTF-8; chardetng recognizes its escape
/// sequences in an all-ASCII file.
fn is_iso_2022_jp(bytes: &[u8]) -> bool {
    let Some(escape) = memchr::memchr(0x1B, bytes) else {
        return false;
    };
    if Encoding::ascii_valid_up_to(bytes) != bytes.len() {
        return false;
    }
    let end = bytes.len().min(escape + ISO_2022_JP_LEN);
    let mut detector = EncodingDetector::new(Iso2022JpDetection::Allow);
    detector.feed(&bytes[..end], end == bytes.len());
    detector.guess(None, Utf8Detection::Allow) == ISO_2022_JP
}

fn guess_legacy(bytes: &[u8]) -> &'static Encoding {
    let first_non_ascii = Encoding::ascii_valid_up_to(bytes);
    let end = bytes.len().min(first_non_ascii.saturating_add(CHARDET_LEN));
    let mut detector = EncodingDetector::new(Iso2022JpDetection::Deny);
    detector.feed(&bytes[..end], end == bytes.len());
    match detector.guess(None, Utf8Detection::Deny) {
        guess if guess == GBK && needs_gb18030(bytes) => GB18030,
        guess => guess,
    }
}

/// chardetng reports GB18030 as GBK. Their decoders agree, but only GB18030's encoder
/// writes four-byte sequences and the two-byte euro sign `A2 E3` back.
fn needs_gb18030(bytes: &[u8]) -> bool {
    let mut index = 0;
    while index < bytes.len() {
        let lead = bytes[index];
        if !(0x81..=0xFE).contains(&lead) {
            index += 1;
            continue;
        }
        match bytes.get(index + 1) {
            Some(0x30..=0x39) => return true,
            Some(0xE3) if lead == 0xA2 => return true,
            _ => index += 2,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use encoding_rs::{BIG5, EUC_KR, SHIFT_JIS, WINDOWS_1251};

    fn utf16(text: &str, little_endian: bool) -> Vec<u8> {
        text.encode_utf16()
            .flat_map(|unit| {
                if little_endian {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                }
            })
            .collect()
    }

    fn legacy(text: &str, encoding: &'static Encoding) -> Vec<u8> {
        let (bytes, _, unmappable) = encoding.encode(text);
        assert!(!unmappable, "{} can't encode the sample", encoding.name());
        bytes.into_owned()
    }

    #[test]
    fn boms_win() {
        let detection = detect(b"\xEF\xBB\xBFabc");
        assert_eq!((detection.encoding, detection.bom), (UTF_8, true));
        assert_eq!(detection.source, DetectionSource::Bom);
        assert_eq!(detect(b"\xFF\xFEa\0").encoding, UTF_16LE);
        assert_eq!(detect(b"\xFE\xFF\0a").encoding, UTF_16BE);
    }

    #[test]
    fn a_utf32_bom_is_not_utf16() {
        let detection = detect(b"\xFF\xFE\0\0a\0\0\0b\0\0\0");
        assert!(!detection.bom);
        assert!(detection.binary);
    }

    #[test]
    fn empty_and_ascii_files_are_utf8() {
        for bytes in [&b""[..], b"plain ascii\n"] {
            let detection = detect(bytes);
            assert_eq!(detection.encoding, UTF_8);
            assert_eq!(detection.source, DetectionSource::ValidUtf8);
            assert!(!detection.binary);
        }
    }

    #[test]
    fn utf16_without_bom_is_recognized_in_both_byte_orders() {
        let text = "Grüße, 世界! A line with a surrogate pair 🎉.\r\nAnd a NUL: \0.\r\n";
        for (little_endian, encoding) in [(true, UTF_16LE), (false, UTF_16BE)] {
            let detection = detect(&utf16(text, little_endian));
            assert_eq!(detection.encoding, encoding);
            assert_eq!(detection.source, DetectionSource::Utf16Heuristic);
            assert!(!detection.bom && !detection.binary);
        }
    }

    #[test]
    fn ascii_and_cyrillic_utf16_are_recognized_though_their_bytes_are_valid_utf8() {
        for text in ["hi\n", "Привет, мир. Как дела у тебя?\nХорошо.\n"]
        {
            let bytes = utf16(text, true);
            assert_eq!(Encoding::utf8_valid_up_to(&bytes), bytes.len());
            assert_eq!(detect(&bytes).encoding, UTF_16LE, "{text:?}");
        }
    }

    #[test]
    fn utf8_text_with_a_few_nuls_is_binary_utf8() {
        let detection = detect(b"abc\0def\nghi\0\n");
        assert_eq!(detection.encoding, UTF_8);
        assert!(detection.binary);
    }

    #[test]
    fn binary_that_is_not_utf8_falls_back_to_windows_1252() {
        let detection = detect(b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\x01\0\xff\xfe\x80");
        assert_eq!(detection.encoding, WINDOWS_1252);
        assert_eq!(detection.source, DetectionSource::Default);
        assert!(detection.binary);
    }

    #[test]
    fn little_endian_integers_are_binary_not_utf16() {
        let bytes: Vec<u8> = (0u16..600).flat_map(|value| value.to_le_bytes()).collect();
        assert!(detect(&bytes).binary);
    }

    #[test]
    fn chardetng_guesses_legacy_encodings() {
        let cases = [
            (
                "Ceci est un fichier en français : déjà, là, où, garçon, œuvre, ça coûte 5 €.\r\n",
                WINDOWS_1252,
            ),
            (
                "Это обычный текст на русском языке, записанный в кодировке Windows-1251.\r\n",
                WINDOWS_1251,
            ),
            (
                "日本語のテキストです。ひらがな、カタカナ、漢字が含まれています。\r\n",
                SHIFT_JIS,
            ),
            (
                "這是一段使用繁體中文寫成的文字，用來測試編碼偵測。\r\n",
                BIG5,
            ),
            (
                "이것은 한국어로 작성된 텍스트입니다. 인코딩 감지를 시험합니다.\r\n",
                EUC_KR,
            ),
        ];
        for (text, encoding) in cases {
            let detection = detect(&legacy(text, encoding));
            assert_eq!(detection.encoding, encoding, "{text}");
            assert_eq!(detection.source, DetectionSource::Chardetng);
        }
    }

    #[test]
    fn gbk_guesses_become_gb18030_when_only_gb18030_can_write_the_bytes_back() {
        let simplified = "这是一段用简体中文写成的文字，用来测试编码检测。\r\n";
        assert_eq!(detect(&legacy(simplified, GBK)).encoding, GBK);
        let four_byte = format!("{simplified}罕见字：𠀀\r\n");
        assert_eq!(detect(&legacy(&four_byte, GB18030)).encoding, GB18030);
    }

    #[test]
    fn iso_2022_jp_is_detected_in_seven_bit_files() {
        let bytes = legacy(
            "日本語のメールです。よろしくお願いします。\r\n",
            ISO_2022_JP,
        );
        assert_eq!(Encoding::ascii_valid_up_to(&bytes), bytes.len());
        assert_eq!(detect(&bytes).encoding, ISO_2022_JP);
    }

    #[test]
    fn ansi_escapes_stay_utf8() {
        let detection = detect(b"\x1b[1;31merror\x1b[0m: something failed\n");
        assert_eq!(detection.encoding, UTF_8);
        assert_eq!(detection.source, DetectionSource::ValidUtf8);
    }
}
