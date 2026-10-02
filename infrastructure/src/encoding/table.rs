use encoding_rs::{
    BIG5_INIT, EUC_JP_INIT, EUC_KR_INIT, Encoding, GB18030_INIT, GBK_INIT, IBM866_INIT,
    ISO_2022_JP_INIT, ISO_8859_2_INIT, ISO_8859_3_INIT, ISO_8859_4_INIT, ISO_8859_5_INIT,
    ISO_8859_6_INIT, ISO_8859_7_INIT, ISO_8859_8_I_INIT, ISO_8859_8_INIT, ISO_8859_10_INIT,
    ISO_8859_13_INIT, ISO_8859_14_INIT, ISO_8859_15_INIT, ISO_8859_16_INIT, KOI8_R_INIT,
    KOI8_U_INIT, MACINTOSH_INIT, SHIFT_JIS_INIT, UTF_8_INIT, UTF_16BE_INIT, UTF_16LE_INIT,
    WINDOWS_874_INIT, WINDOWS_1250_INIT, WINDOWS_1251_INIT, WINDOWS_1252_INIT, WINDOWS_1253_INIT,
    WINDOWS_1254_INIT, WINDOWS_1255_INIT, WINDOWS_1256_INIT, WINDOWS_1257_INIT, WINDOWS_1258_INIT,
    X_MAC_CYRILLIC_INIT, X_USER_DEFINED_INIT,
};

use super::bom_bytes;

/// Sections of the encoding popovers, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EncodingGroup {
    Unicode,
    WesternEuropean,
    CentralEuropean,
    SouthernEuropean,
    Nordic,
    Baltic,
    Celtic,
    Cyrillic,
    Greek,
    Turkish,
    Hebrew,
    Arabic,
    Vietnamese,
    Thai,
    EastAsian,
    Other,
}

impl EncodingGroup {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Unicode => "Unicode",
            Self::WesternEuropean => "Western European",
            Self::CentralEuropean => "Central European",
            Self::SouthernEuropean => "Southern European",
            Self::Nordic => "Nordic",
            Self::Baltic => "Baltic",
            Self::Celtic => "Celtic",
            Self::Cyrillic => "Cyrillic",
            Self::Greek => "Greek",
            Self::Turkish => "Turkish",
            Self::Hebrew => "Hebrew",
            Self::Arabic => "Arabic",
            Self::Vietnamese => "Vietnamese",
            Self::Thai => "Thai",
            Self::EastAsian => "East Asian",
            Self::Other => "Other",
        }
    }
}

/// One choice in the Reinterpret and Convert popovers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodingEntry {
    pub label: &'static str,
    pub encoding: &'static Encoding,
    pub group: EncodingGroup,
    /// Files in this entry start with a byte order mark.
    pub bom: bool,
}

impl EncodingEntry {
    /// A stable name for actions and settings: the encoding's [`Encoding::name`], with
    /// `+bom` for the variants that write a byte order mark.
    pub fn id(&self) -> String {
        if self.bom {
            format!("{}+bom", self.encoding.name())
        } else {
            self.encoding.name().to_owned()
        }
    }

    /// The label without its description, as the status bar shows it: `UTF-8 BOM`,
    /// `UTF-16 LE`, `Windows-1252`, `Shift_JIS`.
    pub fn short_label(&self) -> String {
        let label = self
            .label
            .split_once(" (")
            .map_or(self.label, |(name, _)| name);
        label.replacen(" with BOM", " BOM", 1)
    }
}

/// The entry an [`EncodingEntry::id`] names, in any letter case.
pub fn entry_for_id(id: &str) -> Option<&'static EncodingEntry> {
    let (name, bom) = match id.strip_suffix("+bom") {
        Some(name) => (name, true),
        None => (id, false),
    };
    ENCODINGS
        .iter()
        .find(|entry| entry.bom == bom && entry.encoding.name().eq_ignore_ascii_case(name))
}

/// What the status bar shows for a document's encoding: the entry's short label, or the
/// encoding's name for one that isn't listed.
pub fn status_label(encoding: &'static Encoding, bom: bool) -> String {
    entry_for(encoding, bom).map_or_else(|| encoding.name().to_owned(), EncodingEntry::short_label)
}

const fn entry(
    label: &'static str,
    encoding: &'static Encoding,
    group: EncodingGroup,
    bom: bool,
) -> EncodingEntry {
    EncodingEntry {
        label,
        encoding,
        group,
        bom,
    }
}

use EncodingGroup::{
    Arabic, Baltic, Celtic, CentralEuropean, Cyrillic, EastAsian, Greek, Hebrew, Nordic, Other,
    SouthernEuropean, Thai, Turkish, Unicode, Vietnamese, WesternEuropean,
};

/// Every encoding Stet reads and writes: UTF-8 and UTF-16 with and without a BOM, and every
/// other encoding_rs encoding except `replacement`, which decodes any input to a single
/// U+FFFD. Grouped for display.
pub static ENCODINGS: [EncodingEntry; 42] = [
    entry("UTF-8", &UTF_8_INIT, Unicode, false),
    entry("UTF-8 with BOM", &UTF_8_INIT, Unicode, true),
    entry("UTF-16 LE", &UTF_16LE_INIT, Unicode, false),
    entry("UTF-16 LE with BOM", &UTF_16LE_INIT, Unicode, true),
    entry("UTF-16 BE", &UTF_16BE_INIT, Unicode, false),
    entry("UTF-16 BE with BOM", &UTF_16BE_INIT, Unicode, true),
    entry("Windows-1252", &WINDOWS_1252_INIT, WesternEuropean, false),
    entry("ISO-8859-15", &ISO_8859_15_INIT, WesternEuropean, false),
    entry("Mac Roman", &MACINTOSH_INIT, WesternEuropean, false),
    entry("Windows-1250", &WINDOWS_1250_INIT, CentralEuropean, false),
    entry("ISO-8859-2", &ISO_8859_2_INIT, CentralEuropean, false),
    entry("ISO-8859-16", &ISO_8859_16_INIT, CentralEuropean, false),
    entry("ISO-8859-3", &ISO_8859_3_INIT, SouthernEuropean, false),
    entry("ISO-8859-10", &ISO_8859_10_INIT, Nordic, false),
    entry("Windows-1257", &WINDOWS_1257_INIT, Baltic, false),
    entry("ISO-8859-13", &ISO_8859_13_INIT, Baltic, false),
    entry("ISO-8859-4", &ISO_8859_4_INIT, Baltic, false),
    entry("ISO-8859-14", &ISO_8859_14_INIT, Celtic, false),
    entry("Windows-1251", &WINDOWS_1251_INIT, Cyrillic, false),
    entry("ISO-8859-5", &ISO_8859_5_INIT, Cyrillic, false),
    entry("KOI8-R", &KOI8_R_INIT, Cyrillic, false),
    entry("KOI8-U", &KOI8_U_INIT, Cyrillic, false),
    entry("IBM866 (DOS)", &IBM866_INIT, Cyrillic, false),
    entry("Mac Cyrillic", &X_MAC_CYRILLIC_INIT, Cyrillic, false),
    entry("Windows-1253", &WINDOWS_1253_INIT, Greek, false),
    entry("ISO-8859-7", &ISO_8859_7_INIT, Greek, false),
    entry("Windows-1254", &WINDOWS_1254_INIT, Turkish, false),
    entry("Windows-1255", &WINDOWS_1255_INIT, Hebrew, false),
    entry("ISO-8859-8 (visual)", &ISO_8859_8_INIT, Hebrew, false),
    entry("ISO-8859-8-I (logical)", &ISO_8859_8_I_INIT, Hebrew, false),
    entry("Windows-1256", &WINDOWS_1256_INIT, Arabic, false),
    entry("ISO-8859-6", &ISO_8859_6_INIT, Arabic, false),
    entry("Windows-1258", &WINDOWS_1258_INIT, Vietnamese, false),
    entry("Windows-874 (TIS-620)", &WINDOWS_874_INIT, Thai, false),
    entry("GBK (Simplified Chinese)", &GBK_INIT, EastAsian, false),
    entry(
        "GB18030 (Simplified Chinese)",
        &GB18030_INIT,
        EastAsian,
        false,
    ),
    entry("Big5 (Traditional Chinese)", &BIG5_INIT, EastAsian, false),
    entry("Shift_JIS (Japanese)", &SHIFT_JIS_INIT, EastAsian, false),
    entry("EUC-JP (Japanese)", &EUC_JP_INIT, EastAsian, false),
    entry(
        "ISO-2022-JP (Japanese)",
        &ISO_2022_JP_INIT,
        EastAsian,
        false,
    ),
    entry("EUC-KR (Korean)", &EUC_KR_INIT, EastAsian, false),
    entry("x-user-defined", &X_USER_DEFINED_INIT, Other, false),
];

/// The entry for `encoding`, with or without a BOM; `bom` is ignored for encodings without
/// one. `None` only for `replacement`.
pub fn entry_for(encoding: &'static Encoding, bom: bool) -> Option<&'static EncodingEntry> {
    let bom = bom && !bom_bytes(encoding).is_empty();
    ENCODINGS
        .iter()
        .find(|entry| entry.encoding == encoding && entry.bom == bom)
}

/// Looks an encoding up by the name [`Encoding::name`] gives, as a session stores it, or by
/// any other WHATWG label. Never returns `replacement`.
pub fn encoding_for_name(name: &str) -> Option<&'static Encoding> {
    Encoding::for_label_no_replacement(name.trim().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use encoding_rs::*;
    use std::collections::HashSet;

    const ALL_ENCODINGS: [&Encoding; 40] = [
        BIG5,
        EUC_JP,
        EUC_KR,
        GBK,
        IBM866,
        ISO_2022_JP,
        ISO_8859_10,
        ISO_8859_13,
        ISO_8859_14,
        ISO_8859_15,
        ISO_8859_16,
        ISO_8859_2,
        ISO_8859_3,
        ISO_8859_4,
        ISO_8859_5,
        ISO_8859_6,
        ISO_8859_7,
        ISO_8859_8,
        ISO_8859_8_I,
        KOI8_R,
        KOI8_U,
        SHIFT_JIS,
        UTF_16BE,
        UTF_16LE,
        UTF_8,
        GB18030,
        MACINTOSH,
        REPLACEMENT,
        WINDOWS_1250,
        WINDOWS_1251,
        WINDOWS_1252,
        WINDOWS_1253,
        WINDOWS_1254,
        WINDOWS_1255,
        WINDOWS_1256,
        WINDOWS_1257,
        WINDOWS_1258,
        WINDOWS_874,
        X_MAC_CYRILLIC,
        X_USER_DEFINED,
    ];

    #[test]
    fn covers_every_encoding_but_replacement() {
        for encoding in ALL_ENCODINGS {
            let listed = ENCODINGS.iter().any(|entry| entry.encoding == encoding);
            assert_eq!(listed, encoding != REPLACEMENT, "{}", encoding.name());
        }
    }

    #[test]
    fn only_unicode_entries_have_a_bom_variant() {
        for entry in &ENCODINGS {
            let unicode = !bom_bytes(entry.encoding).is_empty();
            assert_eq!(
                entry.group == EncodingGroup::Unicode,
                unicode,
                "{}",
                entry.label
            );
            assert!(!entry.bom || unicode, "{}", entry.label);
        }
        for encoding in [UTF_8, UTF_16LE, UTF_16BE] {
            assert!(entry_for(encoding, true).is_some_and(|entry| entry.bom));
            assert!(entry_for(encoding, false).is_some_and(|entry| !entry.bom));
        }
    }

    #[test]
    fn labels_and_choices_are_unique() {
        let labels: HashSet<_> = ENCODINGS.iter().map(|entry| entry.label).collect();
        assert_eq!(labels.len(), ENCODINGS.len());
        for entry in &ENCODINGS {
            assert_eq!(entry_for(entry.encoding, entry.bom), Some(entry));
        }
    }

    #[test]
    fn groups_are_contiguous() {
        let mut seen = Vec::new();
        for entry in &ENCODINGS {
            if seen.last() != Some(&entry.group) {
                assert!(!seen.contains(&entry.group), "{:?} is split", entry.group);
                seen.push(entry.group);
            }
        }
    }

    #[test]
    fn ids_name_every_entry_once() {
        let ids: HashSet<String> = ENCODINGS.iter().map(EncodingEntry::id).collect();
        assert_eq!(ids.len(), ENCODINGS.len());
        for entry in &ENCODINGS {
            assert_eq!(entry_for_id(&entry.id()), Some(entry), "{}", entry.label);
        }
        assert_eq!(entry_for(UTF_8, true).unwrap().id(), "UTF-8+bom");
        assert_eq!(entry_for(WINDOWS_1252, false).unwrap().id(), "windows-1252");
        assert_eq!(entry_for_id("windows-1252+bom"), None);
        assert_eq!(entry_for_id("latin1"), None);
        assert_eq!(
            entry_for_id("iso-8859-15").map(EncodingEntry::id),
            Some("ISO-8859-15".to_owned())
        );
    }

    #[test]
    fn status_labels_are_short() {
        let cases = [
            (UTF_8, false, "UTF-8"),
            (UTF_8, true, "UTF-8 BOM"),
            (UTF_16LE, true, "UTF-16 LE BOM"),
            (UTF_16BE, false, "UTF-16 BE"),
            (WINDOWS_1252, false, "Windows-1252"),
            (WINDOWS_1252, true, "Windows-1252"),
            (SHIFT_JIS, false, "Shift_JIS"),
            (GB18030, false, "GB18030"),
            (ISO_8859_8_I, false, "ISO-8859-8-I"),
            (IBM866, false, "IBM866"),
            (REPLACEMENT, false, "replacement"),
        ];
        for (encoding, bom, label) in cases {
            assert_eq!(status_label(encoding, bom), label, "{}", encoding.name());
        }
    }

    #[test]
    fn names_resolve_back_to_their_encoding() {
        for entry in &ENCODINGS {
            assert_eq!(
                encoding_for_name(entry.encoding.name()),
                Some(entry.encoding),
                "{}",
                entry.label
            );
        }
        assert_eq!(encoding_for_name("latin1"), Some(WINDOWS_1252));
        assert_eq!(encoding_for_name("iso-2022-kr"), None);
        assert_eq!(entry_for(REPLACEMENT, false), None);
        assert_eq!(
            entry_for(WINDOWS_1252, true).map(|entry| entry.bom),
            Some(false)
        );
    }
}
