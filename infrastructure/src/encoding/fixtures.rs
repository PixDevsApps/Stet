//! The M3 fixture corpus, built in code rather than committed as binaries.

use encoding_rs::{
    BIG5, EUC_KR, Encoding, GB18030, GBK, ISO_2022_JP, ISO_8859_15, SHIFT_JIS, UTF_8, UTF_16BE,
    UTF_16LE, WINDOWS_1251, WINDOWS_1252,
};
use stet_domain::text::eol::Eol;

pub(crate) struct Fixture {
    pub name: &'static str,
    pub bytes: Vec<u8>,
    /// The detected encoding, or `None` when any guess will do.
    pub encoding: Option<&'static Encoding>,
    pub bom: bool,
    pub binary: bool,
    pub eol: Eol,
    pub mixed_eol: bool,
    /// Expected `lossy_roundtrip`.
    pub lossy: bool,
}

impl Fixture {
    fn new(name: &'static str, bytes: Vec<u8>, encoding: &'static Encoding) -> Self {
        Self {
            name,
            bytes,
            encoding: Some(encoding),
            bom: false,
            binary: false,
            eol: Eol::Lf,
            mixed_eol: false,
            lossy: false,
        }
    }

    fn bom(mut self) -> Self {
        self.bom = true;
        self
    }

    fn binary(mut self) -> Self {
        self.binary = true;
        self
    }

    fn eol(mut self, eol: Eol) -> Self {
        self.eol = eol;
        self
    }

    fn mixed(mut self, dominant: Eol) -> Self {
        self.eol = dominant;
        self.mixed_eol = true;
        self
    }

    fn lossy(mut self) -> Self {
        self.lossy = true;
        self
    }

    fn any_encoding(mut self) -> Self {
        self.encoding = None;
        self
    }
}

pub(crate) fn utf16(text: &str, little_endian: bool) -> Vec<u8> {
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

pub(crate) fn legacy(text: &str, encoding: &'static Encoding) -> Vec<u8> {
    let (bytes, _, unmappable) = encoding.encode(text);
    assert!(!unmappable, "{} can't encode the fixture", encoding.name());
    bytes.into_owned()
}

fn with_prefix(prefix: &[u8], rest: Vec<u8>) -> Vec<u8> {
    [prefix, &rest].concat()
}

/// Deterministic pseudo-random bytes.
pub(crate) fn noise(len: usize, seed: u64) -> Vec<u8> {
    let mut state = seed;
    (0..len)
        .map(|_| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 56) as u8
        })
        .collect()
}

const FRENCH: &str = "Ceci est un fichier en français : déjà, là, où, garçon, œuvre, ça coûte 5 € – « bien sûr ».\r\n\
    Deuxième ligne, avec des accents : élève, forêt, Noël, naïf.\r\n";
const JAPANESE: &str =
    "日本語のテキストです。ひらがな、カタカナ、漢字が含まれています。\r\n二行目もあります。\r\n";
const SIMPLIFIED: &str = "这是一段用简体中文写成的文字，用来测试编码检测。\r\n第二行也在这里。\r\n";

pub(crate) fn fixtures() -> Vec<Fixture> {
    let wide = "Wide text with a NUL \0, a surrogate pair 🎉 and ü.\r\nNext line.\r\n";
    let no_bom = "UTF-16 without a byte order mark, a NUL \0 and 🎉.\nSecond line.\n";
    let mut long_crlf = String::new();
    while long_crlf.len() < 200 * 1024 {
        long_crlf.push_str("A longer line of text that crosses 64 KiB chunk boundaries: é ✓\r\n");
    }
    let mut blob_without_cr = noise(4096, 7);
    blob_without_cr
        .iter_mut()
        .filter(|byte| **byte == b'\r')
        .for_each(|byte| *byte = b'x');
    blob_without_cr[10] = 0;
    vec![
        Fixture::new(
            "utf8",
            "Hello, wörld — ✓ 漢字 🎉\nsecond line\n".into(),
            UTF_8,
        ),
        Fixture::new(
            "utf8-bom",
            with_prefix(b"\xEF\xBB\xBF", "BOM text\r\nwith CRLF\r\n".into()),
            UTF_8,
        )
        .bom()
        .eol(Eol::CrLf),
        Fixture::new(
            "utf16le-bom",
            with_prefix(b"\xFF\xFE", utf16(wide, true)),
            UTF_16LE,
        )
        .bom()
        .eol(Eol::CrLf),
        Fixture::new(
            "utf16be-bom",
            with_prefix(b"\xFE\xFF", utf16(wide, false)),
            UTF_16BE,
        )
        .bom()
        .eol(Eol::CrLf),
        Fixture::new("utf16le", utf16(no_bom, true), UTF_16LE),
        Fixture::new("utf16be", utf16(no_bom, false), UTF_16BE),
        Fixture::new(
            "utf16le-cjk",
            utf16("中文文本，没有字节顺序标记。\r\n第二行。\r\n", true),
            UTF_16LE,
        )
        .eol(Eol::CrLf),
        Fixture::new(
            "utf16le-unpaired-surrogate",
            with_prefix(b"\xFF\xFE", vec![b'a', 0, 0x00, 0xD8, b'b', 0]),
            UTF_16LE,
        )
        .bom()
        .lossy(),
        Fixture::new(
            "utf16be-odd-length",
            with_prefix(b"\xFE\xFF", vec![0, b'a', 0, b'b', 0]),
            UTF_16BE,
        )
        .bom()
        .lossy(),
        Fixture::new("windows-1252", legacy(FRENCH, WINDOWS_1252), WINDOWS_1252).eol(Eol::CrLf),
        Fixture::new(
            "iso-8859-15-reads-as-windows-1252",
            legacy(
                "Le prix est de 10 € pour l'œuvre ; déjà vu, garçon, fenêtre, où.\n",
                ISO_8859_15,
            ),
            WINDOWS_1252,
        ),
        Fixture::new(
            "windows-1251",
            legacy(
                "Это обычный текст на русском языке, записанный в кодировке Windows-1251.\r\n",
                WINDOWS_1251,
            ),
            WINDOWS_1251,
        )
        .eol(Eol::CrLf),
        Fixture::new("shift-jis", legacy(JAPANESE, SHIFT_JIS), SHIFT_JIS).eol(Eol::CrLf),
        Fixture::new(
            "shift-jis-nec-selected-ibm-extension",
            [
                legacy(JAPANESE, SHIFT_JIS),
                b"\x93\xFA\xED\x40\r\n".to_vec(),
            ]
            .concat(),
            SHIFT_JIS,
        )
        .eol(Eol::CrLf)
        .lossy(),
        Fixture::new("gbk", legacy(SIMPLIFIED, GBK), GBK).eol(Eol::CrLf),
        Fixture::new(
            "gb18030-four-byte",
            legacy(&format!("{SIMPLIFIED}罕见字：𠀀，欧元：€。\r\n"), GB18030),
            GB18030,
        )
        .eol(Eol::CrLf),
        Fixture::new(
            "gb18030-single-byte-euro",
            [
                legacy(&format!("{SIMPLIFIED}罕见字：𠀀\r\n"), GB18030),
                vec![0x80],
            ]
            .concat(),
            GB18030,
        )
        .eol(Eol::CrLf)
        .lossy(),
        Fixture::new(
            "big5",
            legacy(
                "這是一段使用繁體中文寫成的文字，用來測試編碼偵測。\r\n",
                BIG5,
            ),
            BIG5,
        )
        .eol(Eol::CrLf),
        Fixture::new(
            "euc-kr",
            legacy(
                "이것은 한국어로 작성된 텍스트입니다. 인코딩 감지를 시험합니다.\r\n",
                EUC_KR,
            ),
            EUC_KR,
        )
        .eol(Eol::CrLf),
        Fixture::new(
            "iso-2022-jp",
            legacy(
                "日本語のメールです。よろしくお願いします。\r\n",
                ISO_2022_JP,
            ),
            ISO_2022_JP,
        )
        .eol(Eol::CrLf),
        Fixture::new(
            "cr-only",
            b"line one\rline two\rline three\r".to_vec(),
            UTF_8,
        )
        .eol(Eol::Cr),
        Fixture::new("crlf", b"a\r\nb\r\n".to_vec(), UTF_8).eol(Eol::CrLf),
        Fixture::new("long-crlf", long_crlf.into_bytes(), UTF_8).eol(Eol::CrLf),
        Fixture::new("mixed-eol", b"one\r\ntwo\nthree\r\nfour\r".to_vec(), UTF_8).mixed(Eol::CrLf),
        Fixture::new("empty", Vec::new(), UTF_8),
        Fixture::new("nul-text", b"abc\0def\nghi\0\n".to_vec(), UTF_8).binary(),
        Fixture::new("placeholder-text", "a␀b\n".into(), UTF_8),
        Fixture::new("placeholder-and-nul", "a␀b\0c\n".into(), UTF_8)
            .binary()
            .lossy(),
        Fixture::new(
            "invalid-utf8",
            b"Mostly UTF-8: d\xC3\xA9j\xC3\xA0 vu, but one byte is broken: \xFF here.\n".to_vec(),
            UTF_8,
        )
        .any_encoding(),
        Fixture::new("binary-without-cr", blob_without_cr, WINDOWS_1252).binary(),
        Fixture::new(
            "binary-png-header",
            b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\x01\0\xff\xfe\x80".to_vec(),
            WINDOWS_1252,
        )
        .binary()
        .mixed(Eol::Lf),
        Fixture::new(
            "utf32le-bom",
            with_prefix(b"\xFF\xFE\0\0", b"h\0\0\0i\0\0\0\n\0\0\0".to_vec()),
            WINDOWS_1252,
        )
        .binary(),
    ]
}
