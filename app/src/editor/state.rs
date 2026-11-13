//! What a document is backed by and how it is written back (ADR-007, ADR-008), in named
//! parts. The parts that keep an invariant (why it is read-only, whether it is loading or
//! saving) change only through the transitions here.

use std::path::PathBuf;
use stet_domain::document::{DiskState, Loss, ReadOnly, TextOrigin, Unwritable};
use stet_domain::session::Fingerprint;
use stet_domain::text::{Eol, EolStats};
use stet_infrastructure::encoding::Encoding;
use stet_infrastructure::fs::{
    DecodeReport, DocInfo, DocumentFormat, EncodingChoice, SaveOutcome, SizeClass,
};

/// What the document is doing with its file. Loading and saving exclude each other: saving
/// waits for a load, and a load that starts during a save takes over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Activity {
    #[default]
    Idle,
    /// Its text is being read or put in: the view takes no edits.
    Loading,
    /// A save is writing a snapshot of its text.
    Saving,
}

/// Mixed line endings in the file as read, and what the user did about them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EolState {
    /// The file had mixed line endings and nothing has normalized them yet.
    pub mixed: bool,
    /// The user chose to keep them for now: the banner is gone, saving still warns.
    pub kept: bool,
}

/// How the next save writes the document, and what the user already agreed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SaveState {
    /// Encoding, BOM and line ending for the next save; Convert changes them.
    pub format: DocumentFormat,
    /// The save format differs from the file's (Convert, a line-ending change).
    pub format_changed: bool,
    /// The user already agreed to save despite a lossy decode or formatted text.
    pub confirmed: bool,
}

/// What the tab is backed by, and how it is written back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocState {
    /// `None` for an untitled document.
    pub path: Option<PathBuf>,
    /// `path` with symlinks resolved, once the file was loaded or saved.
    pub canonical: Option<PathBuf>,
    /// The number of an untitled document.
    pub untitled: Option<u32>,
    /// What a text a comparison opened is called: it has no file, stays out of the
    /// session and closes with the comparison.
    pub temporary: Option<String>,
    /// The name the user gave an untitled document with Rename…, in place of its first
    /// line; a file's name is its file's.
    pub custom_name: Option<String>,
    /// An encoding the user picked with Reinterpret, kept when the file is reloaded.
    pub chosen: Option<EncodingChoice>,
    /// What decoding found when the file was last read, as it found it; `None` for a new
    /// document and a restored backup. What the editor changed since is in `loss`, `eol` and
    /// `save`.
    pub decode: Option<DecodeReport>,
    pub save: SaveState,
    pub eol: EolState,
    /// Why saving can't write back the bytes that were read, until a save writes new ones.
    pub loss: Option<Loss>,
    pub class: SizeClass,
    pub origin: TextOrigin,
    read_only: Option<ReadOnly>,
    pub unwritable: Option<Unwritable>,
    /// The file as last read or written: changes against it are outside changes.
    pub baseline: Option<Fingerprint>,
    pub disk: DiskState,
    /// Highlighting was left off because the file is over the ADR-014 limits.
    pub highlight_limited: bool,
    activity: Activity,
}

impl Default for DocState {
    fn default() -> Self {
        Self {
            path: None,
            canonical: None,
            untitled: None,
            temporary: None,
            custom_name: None,
            chosen: None,
            decode: None,
            save: SaveState::default(),
            eol: EolState::default(),
            loss: None,
            class: SizeClass::Normal,
            origin: TextOrigin::File,
            read_only: None,
            unwritable: None,
            baseline: None,
            disk: DiskState::Same,
            highlight_limited: false,
            activity: Activity::Idle,
        }
    }
}

impl DocState {
    /// A document for the file at `path`, before it is read.
    pub fn file(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            ..Self::default()
        }
    }

    /// Untitled document number `number`.
    pub fn untitled(number: u32) -> Self {
        Self {
            untitled: Some(number),
            ..Self::default()
        }
    }

    /// A read-only text a comparison opened, called `label`.
    pub fn comparison(label: String) -> Self {
        Self {
            temporary: Some(label),
            read_only: Some(ReadOnly::Comparison),
            ..Self::default()
        }
    }

    pub fn read_only(&self) -> Option<ReadOnly> {
        self.read_only
    }

    pub fn is_loading(&self) -> bool {
        self.activity == Activity::Loading
    }

    /// Loading or saving: the file is not to be checked for outside changes now.
    pub fn is_busy_with_file(&self) -> bool {
        self.activity != Activity::Idle
    }

    /// Saving can't reproduce what was read: decoding errors or a lossy encoding.
    pub fn lossy(&self) -> bool {
        self.loss.is_some()
    }

    /// Large-file mode (≥ 50 MiB): no highlighting, wrapping, whitespace or bracket matching.
    pub fn large(&self) -> bool {
        self.class == SizeClass::Large
    }

    /// The line endings of the file as read.
    pub fn eol_stats(&self) -> EolStats {
        self.decode
            .as_ref()
            .map_or_else(EolStats::default, |report| report.eol_stats)
    }

    /// The NUL bytes in the file as read.
    pub fn nul_count(&self) -> usize {
        self.decode.as_ref().map_or(0, |report| report.nul_count)
    }

    /// The file as read had real ␀ characters, which stay as they are.
    pub fn placeholder_conflict(&self) -> bool {
        self.decode
            .as_ref()
            .is_some_and(|report| report.placeholder_conflict)
    }

    pub(super) fn set_activity(&mut self, activity: Activity) {
        self.activity = activity;
    }

    /// Ends `activity` unless another one took over meanwhile.
    pub(super) fn end_activity(&mut self, activity: Activity) {
        if self.activity == activity {
            self.activity = Activity::Idle;
        }
    }

    /// "Edit Anyway": lifts a read-only state that only protects the file.
    pub(super) fn edit_anyway(&mut self) {
        if self.read_only.is_some_and(ReadOnly::can_lift) {
            self.read_only = None;
        }
    }

    /// The document is now the file `info` describes, its text from `origin`: the decode
    /// report, the save format that writes it back, the baseline, and why it is read-only.
    pub(super) fn set_file_info(&mut self, info: &DocInfo, origin: TextOrigin) {
        let report = &info.report;
        self.canonical = Some(info.meta.canonical.clone());
        self.save = SaveState {
            format: report.format(),
            ..SaveState::default()
        };
        self.eol = EolState {
            mixed: report.mixed_eol() && origin == TextOrigin::File,
            kept: false,
        };
        self.loss = Loss::of(report.had_errors, report.lossy_roundtrip);
        self.decode = Some(report.clone());
        self.class = info.class;
        self.origin = origin;
        self.baseline = Some(info.meta.fingerprint());
        self.disk = DiskState::Same;
        self.unwritable = (!info.meta.writable).then_some(Unwritable::Permission);
        self.read_only = if origin == TextOrigin::DisplayBreaks {
            Some(ReadOnly::DisplayBreaks)
        } else if report.binary() {
            Some(ReadOnly::Binary)
        } else if !info.meta.writable {
            Some(ReadOnly::NotWritable)
        } else {
            None
        };
    }

    /// A save wrote `written` to `path` (`outcome`): the file is what the buffer says now.
    pub(super) fn saved(&mut self, path: PathBuf, outcome: &SaveOutcome, written: DocumentFormat) {
        self.path = Some(path);
        self.canonical = Some(outcome.target.clone());
        self.untitled = None;
        self.baseline = Some(outcome.fingerprint);
        self.disk = DiskState::Same;
        self.loss = None;
        self.eol = EolState::default();
        self.origin = TextOrigin::File;
        self.unwritable = None;
        self.save.confirmed = false;
        if self.save.format == written {
            self.save.format_changed = false;
        }
        if self.read_only == Some(ReadOnly::NotWritable) {
            self.read_only = None;
        }
    }

    /// This user may not write the file: it opens read-only, and saving it would fail.
    pub fn deny_writes(&mut self) {
        self.unwritable = Some(Unwritable::Permission);
        self.read_only = Some(ReadOnly::NotWritable);
    }

    /// The next save writes `encoding`, with a BOM when `bom`.
    pub(super) fn convert_encoding(&mut self, encoding: &'static Encoding, bom: bool) {
        let format = &mut self.save.format;
        let changed = format.encoding != encoding || format.bom != bom;
        format.encoding = encoding;
        format.bom = bom;
        self.save.format_changed |= changed;
    }

    /// The next save writes `eol` everywhere, which also settles mixed line endings.
    pub(super) fn convert_eol(&mut self, eol: Eol) {
        let changed = self.save.format.eol != eol || self.eol.mixed;
        self.save.format.eol = eol;
        self.eol = EolState::default();
        self.save.format_changed |= changed;
    }

    /// The file on disk is `now`, and the document takes it as its baseline.
    pub(super) fn settle_disk(&mut self, now: Fingerprint) {
        self.baseline = Some(now);
        self.disk = DiskState::Same;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use stet_domain::ops::pretty::FormatKind;
    use stet_infrastructure::encoding::{Detection, DetectionSource};
    use stet_infrastructure::fs::FileMeta;

    fn report(eol_stats: EolStats) -> DecodeReport {
        let encoding = Encoding::for_label(b"utf-8").unwrap();
        DecodeReport {
            encoding,
            bom: false,
            eol_stats,
            detection: Detection {
                encoding,
                bom: false,
                source: DetectionSource::ValidUtf8,
                binary: false,
            },
            had_errors: false,
            lossy_roundtrip: false,
            nul_count: 0,
            placeholder_conflict: false,
        }
    }

    fn info(report: DecodeReport, writable: bool) -> DocInfo {
        DocInfo {
            report,
            meta: FileMeta {
                size: 10,
                mtime_ns: 1,
                dev: 1,
                ino: 2,
                mode: 0o100_644,
                uid: 1000,
                gid: 1000,
                nlink: 1,
                writable,
                is_symlink: false,
                canonical: PathBuf::from("/tmp/real.txt"),
            },
            class: SizeClass::Normal,
        }
    }

    const MIXED: EolStats = EolStats {
        lf: 2,
        crlf: 1,
        cr: 0,
    };

    #[test]
    fn reading_a_file_sets_why_it_is_read_only() {
        let mut state = DocState::file(PathBuf::from("/tmp/link.txt"));
        state.set_file_info(&info(report(MIXED), true), TextOrigin::File);
        assert_eq!(state.read_only(), None);
        assert_eq!(state.unwritable, None);
        assert!(state.eol.mixed);
        assert_eq!(state.save.format.eol, Eol::Lf);
        assert_eq!(state.canonical.as_deref(), Some("/tmp/real.txt".as_ref()));

        state.set_file_info(&info(report(MIXED), false), TextOrigin::File);
        assert_eq!(state.read_only(), Some(ReadOnly::NotWritable));
        assert_eq!(state.unwritable, Some(Unwritable::Permission));

        let mut binary = report(EolStats::default());
        binary.detection.binary = true;
        state.set_file_info(&info(binary, false), TextOrigin::File);
        assert_eq!(state.read_only(), Some(ReadOnly::Binary));

        state.set_file_info(&info(report(MIXED), true), TextOrigin::DisplayBreaks);
        assert_eq!(state.read_only(), Some(ReadOnly::DisplayBreaks));
        assert!(
            !state.eol.mixed,
            "only the file's own text has its line endings"
        );
    }

    #[test]
    fn edit_anyway_lifts_only_what_protects_the_file() {
        let mut state = DocState::default();
        state.deny_writes();
        state.edit_anyway();
        assert_eq!(state.read_only(), None);
        assert_eq!(state.unwritable, Some(Unwritable::Permission));

        let mut state = DocState::comparison("Clipboard".to_owned());
        state.edit_anyway();
        assert_eq!(state.read_only(), Some(ReadOnly::Comparison));
    }

    #[test]
    fn a_load_that_starts_during_a_save_takes_over() {
        let mut state = DocState::default();
        state.set_activity(Activity::Saving);
        assert!(state.is_busy_with_file() && !state.is_loading());
        state.set_activity(Activity::Loading);
        state.end_activity(Activity::Saving);
        assert!(state.is_loading());
        state.end_activity(Activity::Loading);
        assert!(!state.is_busy_with_file());
    }

    #[test]
    fn converting_changes_the_save_format_once_it_differs() {
        let mut state = DocState::default();
        state.convert_eol(Eol::Lf);
        assert!(!state.save.format_changed);
        state.eol.mixed = true;
        state.convert_eol(Eol::Lf);
        assert!(state.save.format_changed);
        assert_eq!(state.eol, EolState::default());

        let mut state = DocState::default();
        let utf8 = state.save.format.encoding;
        state.convert_encoding(utf8, false);
        assert!(!state.save.format_changed);
        state.convert_encoding(utf8, true);
        assert!(state.save.format_changed && state.save.format.bom);
    }

    #[test]
    fn saving_settles_what_reading_found() {
        let mut state = DocState::file(PathBuf::from("/tmp/notes.json"));
        let mut lossy = report(MIXED);
        lossy.had_errors = true;
        state.set_file_info(&info(lossy, false), TextOrigin::Formatted(FormatKind::Json));
        state.save.confirmed = true;
        state.convert_encoding(state.save.format.encoding, true);
        let written = state.save.format;
        let outcome = SaveOutcome {
            fingerprint: info(report(MIXED), true).meta.fingerprint(),
            method: stet_infrastructure::fs::SaveMethod::AtomicRename,
            target: PathBuf::from("/tmp/real.json"),
        };
        state.saved(PathBuf::from("/tmp/notes.json"), &outcome, written);
        assert_eq!(state.loss, None);
        assert_eq!(state.origin, TextOrigin::File);
        assert_eq!(state.read_only(), None);
        assert_eq!(state.unwritable, None);
        assert!(!state.save.confirmed && !state.save.format_changed);
        assert_eq!(state.disk, DiskState::Same);
        assert_eq!(
            state.nul_count(),
            0,
            "the decode report stays what reading found"
        );
        assert!(
            state
                .decode
                .as_ref()
                .is_some_and(|report| report.had_errors)
        );
    }
}
