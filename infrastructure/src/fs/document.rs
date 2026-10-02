use std::path::Path;

use encoding_rs::{Encoding, UTF_8};
use stet_domain::text::eol::{Eol, EolStats};

use super::load::{LoadError, load};
use super::meta::FileMeta;
use super::save::{SaveError, SaveOptions, SaveOutcome, safe_save};
use super::size::{SizeClass, SizePolicy};
use crate::encoding::{
    Detection, EncodingEntry, Unmappable, decode_to_lf, detect, encode_with_eol,
};

/// How a document is written back: what the status bar shows and Convert changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentFormat {
    pub encoding: &'static Encoding,
    pub bom: bool,
    pub eol: Eol,
    /// Write every placeholder as NUL. Off when the file already had real U+2400
    /// characters, which then stay as they are.
    pub map_placeholder: bool,
}

impl Default for DocumentFormat {
    /// UTF-8 without a BOM and LF, for new documents.
    fn default() -> Self {
        Self {
            encoding: UTF_8,
            bom: false,
            eol: Eol::Lf,
            map_placeholder: true,
        }
    }
}

/// An encoding the user picked over the detected one (Reinterpret).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EncodingChoice {
    pub encoding: &'static Encoding,
    pub bom: bool,
}

impl From<&EncodingEntry> for EncodingChoice {
    fn from(entry: &EncodingEntry) -> Self {
        Self {
            encoding: entry.encoding,
            bom: entry.bom,
        }
    }
}

/// A file's bytes decoded for the buffer, and everything the banners need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedDocument {
    /// LF-normalized, with NUL mapped to the placeholder: ready to insert.
    pub text: String,
    pub encoding: &'static Encoding,
    pub bom: bool,
    /// The dominant line ending; LF when there is none.
    pub eol: Eol,
    /// More than one kind of line ending. Saving writes `eol` everywhere.
    pub mixed_eol: bool,
    pub eol_stats: EolStats,
    /// What detection found, also when a [`EncodingChoice`] overrode it.
    pub detection: Detection,
    pub had_errors: bool,
    pub lossy_roundtrip: bool,
    pub nul_count: usize,
    pub placeholder_conflict: bool,
    /// Detection's verdict on the bytes, also under an [`EncodingChoice`]: open read-only.
    pub binary: bool,
}

/// [`DecodedDocument`] plus the file's metadata and size class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedDocument {
    /// LF-normalized, with NUL mapped to the placeholder: ready to insert.
    pub text: String,
    pub encoding: &'static Encoding,
    pub bom: bool,
    /// The dominant line ending; LF when there is none.
    pub eol: Eol,
    /// More than one kind of line ending. Saving writes `eol` everywhere; warn first.
    pub mixed_eol: bool,
    pub eol_stats: EolStats,
    /// What detection found, also when a [`EncodingChoice`] overrode it.
    pub detection: Detection,
    /// Malformed bytes were replaced with U+FFFD.
    pub had_errors: bool,
    /// Saving in this encoding would not reproduce the file.
    pub lossy_roundtrip: bool,
    pub nul_count: usize,
    pub placeholder_conflict: bool,
    /// Detection's verdict on the bytes, also under an [`EncodingChoice`]: open read-only.
    pub binary: bool,
    /// Taken before reading; its fingerprint is the reload baseline.
    pub meta: FileMeta,
    pub class: SizeClass,
}

macro_rules! document_format {
    ($document:ty) => {
        impl $document {
            /// The format that writes this document back as it was read.
            pub fn format(&self) -> DocumentFormat {
                DocumentFormat {
                    encoding: self.encoding,
                    bom: self.bom,
                    eol: self.eol,
                    map_placeholder: !self.placeholder_conflict,
                }
            }

            /// Saving without edits reproduces the file byte for byte: nothing was lost in
            /// decoding and the line endings weren't mixed.
            pub fn round_trips(&self) -> bool {
                !self.lossy_roundtrip && !self.mixed_eol
            }
        }
    };
}

document_format!(DecodedDocument);
document_format!(OpenedDocument);

/// Detects the encoding, unless `choice` overrides it, and decodes, maps NUL and normalizes
/// the line endings in one pass. Pure, for callers that already hold the bytes.
pub fn decode_document(bytes: &[u8], choice: Option<EncodingChoice>) -> DecodedDocument {
    let detection = detect(bytes);
    let (encoding, bom) = choice.map_or((detection.encoding, detection.bom), |choice| {
        (choice.encoding, choice.bom)
    });
    let (decoded, eol_stats) = decode_to_lf(bytes, encoding, bom);
    DecodedDocument {
        text: decoded.text,
        encoding,
        bom,
        eol: eol_stats.dominant().unwrap_or_default(),
        mixed_eol: eol_stats.is_mixed(),
        eol_stats,
        detection,
        had_errors: decoded.had_errors,
        lossy_roundtrip: decoded.lossy_roundtrip,
        nul_count: decoded.nul_count,
        placeholder_conflict: decoded.placeholder_conflict,
        binary: detection.binary,
    }
}

/// Encodes LF-normalized buffer text in `format`: LF becomes the document's line ending,
/// placeholders become NUL when the format says so, and characters the encoding lacks fail
/// the encode, with character offsets into `text`.
pub fn encode_document(text: &str, format: &DocumentFormat) -> Result<Vec<u8>, Unmappable> {
    encode_with_eol(
        text,
        format.encoding,
        format.bom,
        format.map_placeholder,
        format.eol,
    )
}

/// Loads and decodes a file for a new tab. Call it on a worker thread.
pub fn open_document(path: &Path, policy: &SizePolicy) -> Result<OpenedDocument, LoadError> {
    open(path, policy, None)
}

/// Reads the file again as `choice` (Reinterpret).
pub fn open_document_as(
    path: &Path,
    policy: &SizePolicy,
    choice: EncodingChoice,
) -> Result<OpenedDocument, LoadError> {
    open(path, policy, Some(choice))
}

fn open(
    path: &Path,
    policy: &SizePolicy,
    choice: Option<EncodingChoice>,
) -> Result<OpenedDocument, LoadError> {
    let loaded = load(path, policy)?;
    let document = decode_document(&loaded.bytes, choice);
    drop(loaded.bytes);
    Ok(OpenedDocument {
        text: document.text,
        encoding: document.encoding,
        bom: document.bom,
        eol: document.eol,
        mixed_eol: document.mixed_eol,
        eol_stats: document.eol_stats,
        detection: document.detection,
        had_errors: document.had_errors,
        lossy_roundtrip: document.lossy_roundtrip,
        nul_count: document.nul_count,
        placeholder_conflict: document.placeholder_conflict,
        binary: document.binary,
        meta: loaded.meta,
        class: loaded.class,
    })
}

/// Encodes buffer text in `format` and writes it with [`safe_save`]. Call it on a worker
/// thread with a snapshot of the buffer.
pub fn save_document(
    path: &Path,
    text: &str,
    format: &DocumentFormat,
    options: &SaveOptions,
) -> Result<SaveOutcome, SaveError> {
    let bytes = encode_document(text, format)?;
    safe_save(path, &bytes, options)
}

/// Splits `text` into pieces of at most `max_bytes`, each ending after a line break where
/// the window has one, for inserting a large document in steps. Never splits a character;
/// a piece is longer than `max_bytes` only when one character is.
pub fn text_chunks(text: &str, max_bytes: usize) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let limit = rest.floor_char_boundary(max_bytes);
        let end = if limit == rest.len() {
            limit
        } else {
            match memchr::memrchr(b'\n', &rest.as_bytes()[..limit]) {
                Some(newline) => newline + 1,
                None if limit > 0 => limit,
                None => rest.ceil_char_boundary(1),
            }
        };
        let (chunk, tail) = rest.split_at(end);
        rest = tail;
        Some(chunk)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::fixtures::{fixtures, legacy};
    use crate::encoding::{ENCODINGS, PLACEHOLDER, decode_to_lf};
    use crate::fs::SaveMethod;
    use encoding_rs::{ISO_8859_15, SHIFT_JIS, UTF_16LE, WINDOWS_1252};
    use proptest::prelude::*;
    use std::fs;
    use std::time::{Duration, Instant};

    fn save_options(dir: &Path) -> SaveOptions {
        SaveOptions::new(dir.join("backups"))
    }

    #[test]
    fn every_fixture_round_trips_or_is_flagged() {
        let dir = tempfile::tempdir().unwrap();
        let policy = SizePolicy::default();
        for fixture in fixtures() {
            let path = dir.path().join(fixture.name);
            fs::write(&path, &fixture.bytes).unwrap();
            let opened = open_document(&path, &policy).unwrap();
            let name = fixture.name;
            if let Some(encoding) = fixture.encoding {
                assert_eq!(opened.encoding.name(), encoding.name(), "{name}");
            }
            assert_eq!(opened.bom, fixture.bom, "{name}: bom");
            assert_eq!(opened.binary, fixture.binary, "{name}: binary");
            assert_eq!(opened.eol, fixture.eol, "{name}: eol");
            assert_eq!(opened.mixed_eol, fixture.mixed_eol, "{name}: mixed");
            assert_eq!(opened.lossy_roundtrip, fixture.lossy, "{name}: lossy");
            assert!(
                !opened.text.contains('\0'),
                "{name}: NUL in the buffer text"
            );
            assert!(!opened.text.contains('\r'), "{name}: CR in the buffer text");

            let saved_path = dir.path().join(format!("{name}.saved"));
            match save_document(
                &saved_path,
                &opened.text,
                &opened.format(),
                &save_options(dir.path()),
            ) {
                Ok(_) => {
                    let saved = fs::read(&saved_path).unwrap();
                    assert_eq!(
                        saved == fixture.bytes,
                        opened.round_trips(),
                        "{name}: exactness"
                    );
                    if opened.mixed_eol && !opened.lossy_roundtrip {
                        let reopened = open_document(&saved_path, &policy).unwrap();
                        assert_eq!(reopened.text, opened.text, "{name}");
                        assert!(!reopened.mixed_eol, "{name}");
                        assert_eq!(reopened.eol, opened.eol, "{name}");
                    }
                }
                Err(SaveError::Unmappable(_)) => assert!(opened.lossy_roundtrip, "{name}"),
                Err(error) => panic!("{name}: {error}"),
            }
        }
    }

    #[test]
    fn reinterpret_reads_the_file_again_in_another_encoding() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("euro.txt");
        let bytes = legacy("Prix : 10 € pour l'œuvre.\n", ISO_8859_15);
        fs::write(&path, &bytes).unwrap();
        let policy = SizePolicy::default();

        let detected = open_document(&path, &policy).unwrap();
        assert_eq!(detected.encoding, WINDOWS_1252);
        assert_eq!(detected.text, "Prix : 10 ¤ pour l'½uvre.\n");

        let choice = EncodingChoice {
            encoding: ISO_8859_15,
            bom: false,
        };
        let reread = open_document_as(&path, &policy, choice).unwrap();
        assert_eq!(reread.text, "Prix : 10 € pour l'œuvre.\n");
        assert_eq!(reread.detection.encoding, WINDOWS_1252);
        assert!(reread.round_trips());
        let saved = dir.path().join("saved.txt");
        save_document(
            &saved,
            &reread.text,
            &reread.format(),
            &save_options(dir.path()),
        )
        .unwrap();
        assert_eq!(fs::read(&saved).unwrap(), bytes);
    }

    #[test]
    fn convert_writes_another_encoding_and_line_ending() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("doc.txt");
        let format = DocumentFormat {
            encoding: UTF_16LE,
            bom: true,
            eol: Eol::CrLf,
            map_placeholder: true,
        };
        save_document(&path, "æøå\nsecond␀\n", &format, &save_options(dir.path())).unwrap();
        let opened = open_document(&path, &SizePolicy::default()).unwrap();
        assert_eq!(opened.format(), format);
        assert_eq!(opened.text, "æøå\nsecond␀\n");
        assert_eq!(opened.nul_count, 1);
        assert!(opened.round_trips());
    }

    #[test]
    fn unmappable_characters_stop_the_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sjis.txt");
        fs::write(&path, "untouched").unwrap();
        let format = DocumentFormat {
            encoding: SHIFT_JIS,
            ..DocumentFormat::default()
        };
        let error =
            save_document(&path, "日本 Ω €\n", &format, &save_options(dir.path())).unwrap_err();
        let SaveError::Unmappable(unmappable) = error else {
            panic!("expected Unmappable, got {error}");
        };
        assert_eq!(unmappable.chars, vec![(5, '€')]);
        assert_eq!(fs::read_to_string(&path).unwrap(), "untouched");
    }

    #[test]
    fn placeholder_conflicts_keep_real_u2400() {
        let decoded = decode_document("a␀b\n".as_bytes(), None);
        assert!(decoded.placeholder_conflict && !decoded.format().map_placeholder);
        assert_eq!(
            encode_document(&decoded.text, &decoded.format()).unwrap(),
            "a␀b\n".as_bytes()
        );
        let nul = decode_document(b"a\0b\n", None);
        assert_eq!(nul.text, format!("a{PLACEHOLDER}b\n"));
        assert_eq!(
            encode_document(&nul.text, &nul.format()).unwrap(),
            b"a\0b\n"
        );
    }

    #[test]
    fn every_listed_encoding_round_trips_its_own_output() {
        let text = "Line one\nLine two with digits 0123 and punctuation: ;.,!?\n";
        for entry in &ENCODINGS {
            let format = DocumentFormat {
                encoding: entry.encoding,
                bom: entry.bom,
                eol: Eol::CrLf,
                map_placeholder: true,
            };
            let bytes = encode_document(text, &format).unwrap();
            let decoded = decode_document(&bytes, Some(entry.into()));
            assert_eq!(decoded.text, text, "{}", entry.label);
            assert_eq!(decoded.eol, Eol::CrLf, "{}", entry.label);
            assert!(decoded.round_trips(), "{}", entry.label);
        }
    }

    #[test]
    fn chunks_end_at_line_breaks_and_never_split_characters() {
        let text = "ab\ncd\nééé\n";
        let chunks: Vec<&str> = text_chunks(text, 5).collect();
        assert_eq!(chunks, ["ab\n", "cd\n", "éé", "é\n"]);
        assert_eq!(text_chunks("🎉🎉", 1).collect::<Vec<_>>(), ["🎉", "🎉"]);
        assert_eq!(text_chunks("", 4).count(), 0);
    }

    proptest! {
        #[test]
        fn chunks_cover_the_text(text in "(\\PC|\n){0,200}", max_bytes in 1usize..32) {
            let chunks: Vec<&str> = text_chunks(&text, max_bytes).collect();
            prop_assert_eq!(chunks.concat(), text.as_str());
            for chunk in &chunks {
                prop_assert!(!chunk.is_empty());
                prop_assert!(chunk.len() <= max_bytes || chunk.chars().count() == 1);
            }
        }

        #[test]
        fn documents_round_trip_in_every_line_ending(
            text in "([^\r\n\u{0}\u{2400}]{0,12}\n){0,20}",
            eol in prop_oneof![Just(Eol::Lf), Just(Eol::CrLf), Just(Eol::Cr)],
            index in 0..6usize,
        ) {
            let entry = &ENCODINGS[index];
            let format = DocumentFormat { encoding: entry.encoding, bom: entry.bom, eol, map_placeholder: true };
            let bytes = encode_document(&text, &format).unwrap();
            let decoded = decode_document(&bytes, Some(entry.into()));
            prop_assert_eq!(&decoded.text, &text);
            prop_assert!(decoded.round_trips());
            if text.contains('\n') {
                prop_assert_eq!(decoded.eol, eol);
            }
            prop_assert_eq!(encode_document(&decoded.text, &decoded.format()).unwrap(), bytes);
        }
    }

    fn best_of<T>(runs: usize, mut run: impl FnMut() -> T) -> (Duration, T) {
        let mut best = Duration::MAX;
        let mut last = None;
        for _ in 0..runs {
            let start = Instant::now();
            let result = run();
            best = best.min(start.elapsed());
            last = Some(result);
        }
        (best, last.unwrap())
    }

    fn report(label: &str, bytes: usize, time: Duration) {
        let mib = bytes as f64 / (1024.0 * 1024.0);
        eprintln!(
            "{label}: {mib:.1} MiB in {:.1} ms ({:.0} MiB/s)",
            time.as_secs_f64() * 1000.0,
            mib / time.as_secs_f64()
        );
    }

    /// `cargo test -p stet-infrastructure --release -- --ignored --nocapture bench_`; the
    /// file benchmarks write under `STET_BENCH_DIR` when set, else the temporary directory.
    #[test]
    #[ignore = "benchmark"]
    fn bench_decode_and_normalize() {
        const TARGET: usize = 100 * 1024 * 1024;
        let line = "2026-10-01T12:00:00Z INFO request id=42 path=/api/items/7 user=\"müller\" ✓ done in 12 ms\n";
        let utf8 = line.repeat(TARGET.div_ceil(line.len()));
        let crlf = utf8.replace('\n', "\r\n");
        let french = legacy(
            "Ceci est une ligne en français : déjà, garçon, œuvre, 5 € – naïveté.\r\n",
            WINDOWS_1252,
        );
        let latin = french.repeat((TARGET / 10).div_ceil(french.len()));

        let (time, detection) = best_of(5, || detect(utf8.as_bytes()));
        assert_eq!(detection.encoding, UTF_8);
        report("detect, UTF-8 LF", utf8.len(), time);
        let (time, (decoded, _)) = best_of(5, || decode_to_lf(utf8.as_bytes(), UTF_8, false));
        assert!(!decoded.lossy_roundtrip);
        report("decode + normalize, UTF-8 LF", utf8.len(), time);
        drop(decoded);
        let (time, document) = best_of(5, || decode_document(utf8.as_bytes(), None));
        assert!(document.round_trips() && document.eol == Eol::Lf);
        report("detect + decode + normalize, UTF-8 LF", utf8.len(), time);
        drop(document);
        let (time, document) = best_of(5, || decode_document(crlf.as_bytes(), None));
        assert!(document.round_trips() && document.eol == Eol::CrLf);
        report("detect + decode + normalize, UTF-8 CRLF", crlf.len(), time);
        drop(document);

        let (time, detection) = best_of(5, || detect(&latin));
        assert_eq!(detection.encoding, WINDOWS_1252);
        report("detect, Windows-1252 CRLF", latin.len(), time);
        let (time, (decoded, _)) = best_of(5, || decode_to_lf(&latin, WINDOWS_1252, false));
        assert!(!decoded.lossy_roundtrip);
        report(
            "decode + normalize + round-trip check, Windows-1252 CRLF",
            latin.len(),
            time,
        );
        let (time, document) = best_of(5, || decode_document(&latin, None));
        assert!(document.round_trips() && document.encoding == WINDOWS_1252);
        report(
            "detect + decode + normalize, Windows-1252 CRLF",
            latin.len(),
            time,
        );

        let base = std::env::var_os("STET_BENCH_DIR").map_or_else(std::env::temp_dir, Into::into);
        let dir = tempfile::tempdir_in(base).unwrap();
        let path = dir.path().join("big.log");
        fs::write(&path, &utf8).unwrap();
        let policy = SizePolicy::default();
        let (time, opened) = best_of(5, || open_document(&path, &policy).unwrap());
        assert_eq!(opened.class, SizeClass::Large);
        report(
            "open_document (read + detect + decode), UTF-8 LF",
            utf8.len(),
            time,
        );
        let format = opened.format();
        let (time, _) = best_of(3, || encode_document(&opened.text, &format).unwrap());
        report("encode_document, UTF-8 LF", utf8.len(), time);
        let crlf_format = DocumentFormat {
            eol: Eol::CrLf,
            ..format
        };
        let (time, _) = best_of(3, || encode_document(&opened.text, &crlf_format).unwrap());
        report("encode_document, UTF-8 to CRLF", utf8.len(), time);
        let options = save_options(dir.path());
        let (time, outcome) = best_of(3, || {
            save_document(&path, &opened.text, &format, &options).unwrap()
        });
        assert_eq!(outcome.method, SaveMethod::AtomicRename);
        report(
            "save_document (encode + fsync + rename), UTF-8 LF",
            utf8.len(),
            time,
        );
        eprintln!("file benchmarks ran in {}", dir.path().display());
    }

    fn peak_rss() -> u64 {
        fs::read_to_string("/proc/self/status")
            .unwrap()
            .lines()
            .find_map(|line| line.strip_prefix("VmHWM:"))
            .and_then(|value| value.trim().strip_suffix("kB"))
            .and_then(|kib| kib.trim().parse::<u64>().ok())
            .unwrap()
            * 1024
    }

    /// Run alone, since the peak covers the whole process:
    /// `cargo test -p stet-infrastructure --release -- --ignored --nocapture --exact
    /// fs::document::tests::bench_open_peak_memory`
    #[test]
    #[ignore = "benchmark"]
    fn bench_open_peak_memory() {
        use std::io::Write;
        let base = std::env::var_os("STET_BENCH_DIR").map_or_else(std::env::temp_dir, Into::into);
        let dir = tempfile::tempdir_in(base).unwrap();
        let path = dir.path().join("big.log");
        let line = "2026-10-01T12:00:00Z INFO request id=42 path=/api/items/7 user=\"müller\" ✓ done in 12 ms\r\n";
        let mut file = std::io::BufWriter::new(fs::File::create(&path).unwrap());
        let mut size = 0;
        while size < 100 * 1024 * 1024 {
            file.write_all(line.as_bytes()).unwrap();
            size += line.len();
        }
        file.flush().unwrap();
        let before = peak_rss();
        let opened = open_document(&path, &SizePolicy::default()).unwrap();
        let after = peak_rss();
        assert!(opened.round_trips());
        eprintln!(
            "open_document of {:.1} MiB (CRLF): peak RSS grew by {:.1} MiB ({:.2}x the file); the text is {:.1} MiB",
            size as f64 / 1048576.0,
            (after - before) as f64 / 1048576.0,
            (after - before) as f64 / size as f64,
            opened.text.len() as f64 / 1048576.0
        );
    }
}
