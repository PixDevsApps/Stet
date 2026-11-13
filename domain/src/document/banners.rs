//! The banners above a tab's text (DESIGN.md "Window layout"): which ones a document's state
//! calls for, what they say and which buttons they offer, most urgent first. The app fills
//! [`BannerFacts`] from the document and renders the [`BannerSpec`]s; what each button does is
//! the app's.

use std::path::PathBuf;

use super::state::{DiskState, Loss, ReadOnly, TextOrigin, Unwritable};
use super::{MIB, sudoedit_command};
use crate::text::{DISPLAY_BREAK_CHARS, Eol, EolStats, grouped};

/// What a banner is about. Banners stack in this order, the most urgent on top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BannerKind {
    /// Reading the file into the tab, with its progress.
    Loading,
    /// The file changed on disk or is gone.
    Disk,
    /// What restoring the session found: a file changed since its backup, or unsaved
    /// changes that could not be restored.
    Restore,
    /// The tab refuses edits ([`ReadOnly`]) or the last save could not write the file.
    ReadOnly,
    /// The file contains NUL bytes.
    Binary,
    /// Saving can't give back the bytes that were read ([`Loss`]).
    Lossy,
    /// The file has both NUL bytes and real ␀ characters, which save alike.
    Placeholder,
    /// The file mixes line endings.
    MixedEol,
    /// Long lines were broken for display or pretty-printed.
    LongLines,
    /// The file is open in large-file mode.
    LargeFile,
    /// The size limits held syntax highlighting back.
    Highlighting,
}

impl BannerKind {
    /// The kinds [`banners_for`] decides; the others (loading progress, the session restore)
    /// are shown and hidden by what they report on.
    pub const FROM_STATE: [Self; 9] = [
        Self::Disk,
        Self::ReadOnly,
        Self::Binary,
        Self::Lossy,
        Self::Placeholder,
        Self::MixedEol,
        Self::LongLines,
        Self::LargeFile,
        Self::Highlighting,
    ];
}

/// A banner button: what it asks the app to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BannerAction {
    /// Reload the file, replacing the unsaved changes.
    Reload,
    /// Reload, for a restored backup whose file changed while Stet was closed.
    LoadDiskVersion,
    /// Keep the unsaved changes; the next save overwrites the file.
    KeepMine,
    /// Compare the unsaved text with the file on disk.
    Compare,
    /// Save the document whose file is gone.
    Save,
    /// Copy this command, which edits the file as root.
    CopyCommand(String),
    /// Lift the read-only state.
    EditAnyway,
    /// Read the file again in another encoding.
    Reinterpret,
    /// Make the next save write this line ending everywhere.
    Normalize(Eol),
    /// Keep the mixed line endings for now.
    KeepMixedEndings,
    /// Turn on the highlighting the size limits held back.
    HighlightAnyway,
}

impl BannerAction {
    /// The button's label.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Reload => "Reload".to_owned(),
            Self::LoadDiskVersion => "Load Disk Version".to_owned(),
            Self::KeepMine => "Keep Mine".to_owned(),
            Self::Compare => "Compare".to_owned(),
            Self::Save => "Save".to_owned(),
            Self::CopyCommand(_) => "Copy Command".to_owned(),
            Self::EditAnyway => "Edit Anyway".to_owned(),
            Self::Reinterpret => "Reinterpret…".to_owned(),
            Self::Normalize(eol) => format!("Normalize to {}", eol.label()),
            Self::KeepMixedEndings => "Keep".to_owned(),
            Self::HighlightAnyway => "Highlight Anyway".to_owned(),
        }
    }
}

/// One banner to show: its kind, its text and its buttons in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BannerSpec {
    /// Where the banner stacks.
    pub kind: BannerKind,
    /// The sentence the banner shows.
    pub title: String,
    /// The buttons, left to right.
    pub actions: Vec<BannerAction>,
}

/// The highlighting limits of `config.toml` (ADR-014).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighlightLimits {
    /// Highlighting stays off for a file of more bytes than this.
    pub max_bytes: usize,
    /// Highlighting stays off for a file of more lines than this.
    pub max_lines: usize,
}

/// What the banners need to know about a document.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BannerFacts {
    /// The name the tab shows.
    pub name: String,
    /// The file, for the sudoedit command.
    pub path: Option<PathBuf>,
    /// How the file on disk compares with the document.
    pub disk: DiskState,
    /// The changed file is a restored backup's, whose file changed while Stet was closed.
    pub restored_conflict: bool,
    /// Why the tab refuses edits, if it does.
    pub read_only: Option<ReadOnly>,
    /// Why the last save could not write the file, if it failed.
    pub unwritable: Option<Unwritable>,
    /// Why saving can't give back the bytes that were read, if it can't.
    pub loss: Option<Loss>,
    /// The save encoding as the status bar shows it.
    pub encoding: String,
    /// NUL bytes in the file as read.
    pub nul_count: usize,
    /// The file as read had real ␀ characters as well.
    pub placeholder_conflict: bool,
    /// The file's line endings are mixed and nothing has normalised them.
    pub mixed_eol: bool,
    /// The user chose to keep the mixed line endings for now.
    pub mixed_kept: bool,
    /// The count of each line ending in the file as read.
    pub eol_stats: EolStats,
    /// The line ending the next save writes.
    pub eol: Eol,
    /// Where the buffer's text came from.
    pub origin: TextOrigin,
    /// The file's size in bytes, in large-file mode.
    pub large_file: Option<u64>,
    /// The limits that held highlighting back, while it is off for them and the document has
    /// a language.
    pub highlight_limited: Option<HighlightLimits>,
}

/// The banners `facts` call for, most urgent first: one at most of each of
/// [`BannerKind::FROM_STATE`].
#[must_use]
pub fn banners_for(facts: &BannerFacts) -> Vec<BannerSpec> {
    let banners = [
        disk(facts),
        read_only(facts),
        binary(facts),
        lossy(facts),
        placeholder(facts),
        mixed_eol(facts),
        long_lines(facts),
        large_file(facts),
        highlighting(facts),
    ];
    banners.into_iter().flatten().collect()
}

fn spec(kind: BannerKind, title: String, actions: Vec<BannerAction>) -> BannerSpec {
    BannerSpec {
        kind,
        title,
        actions,
    }
}

fn disk(facts: &BannerFacts) -> Option<BannerSpec> {
    let name = &facts.name;
    match facts.disk {
        DiskState::Same => None,
        // A restored backup whose file changed while Stet was closed says so.
        DiskState::Changed if facts.restored_conflict => Some(spec(
            BannerKind::Disk,
            format!("The file “{name}” changed on disk since your unsaved changes."),
            vec![
                BannerAction::Compare,
                BannerAction::KeepMine,
                BannerAction::LoadDiskVersion,
            ],
        )),
        DiskState::Changed => Some(spec(
            BannerKind::Disk,
            format!("“{name}” changed on disk."),
            vec![
                BannerAction::Compare,
                BannerAction::Reload,
                BannerAction::KeepMine,
            ],
        )),
        DiskState::Deleted { .. } => Some(spec(
            BannerKind::Disk,
            format!("“{name}” is no longer on disk. Save it to keep it."),
            vec![BannerAction::Save],
        )),
    }
}

fn read_only(facts: &BannerFacts) -> Option<BannerSpec> {
    let name = &facts.name;
    let path = facts
        .path
        .as_deref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let command = sudoedit_command(&path);
    if facts.read_only == Some(ReadOnly::NotWritable) {
        return Some(spec(
            BannerKind::ReadOnly,
            format!(
                "You may not write “{name}”, so it is open read-only. To edit it as root, \
                 run: {command}"
            ),
            vec![BannerAction::CopyCommand(command), BannerAction::EditAnyway],
        ));
    }
    match facts.unwritable? {
        Unwritable::Permission => Some(spec(
            BannerKind::ReadOnly,
            format!(
                "You may not write “{name}”. Save a copy with Save As, or edit it as root: \
                 {command}"
            ),
            vec![BannerAction::CopyCommand(command)],
        )),
        Unwritable::ReadOnlyFilesystem => Some(spec(
            BannerKind::ReadOnly,
            format!("“{name}” is on a read-only file system. Save a copy with Save As."),
            Vec::new(),
        )),
    }
}

fn binary(facts: &BannerFacts) -> Option<BannerSpec> {
    if facts.read_only != Some(ReadOnly::Binary) {
        return None;
    }
    let name = &facts.name;
    if facts.loss.is_some() || facts.mixed_eol {
        return Some(spec(
            BannerKind::Binary,
            format!(
                "“{name}” looks binary and can't be saved exactly as it was, so it is \
                 open read-only."
            ),
            Vec::new(),
        ));
    }
    Some(spec(
        BannerKind::Binary,
        format!(
            "“{name}” looks binary (it has NUL bytes), so it is open read-only. NUL bytes \
             show as ␀ and are saved as NUL."
        ),
        vec![BannerAction::EditAnyway],
    ))
}

/// Both NUL bytes and real ␀ characters: the ␀ banner says it, and stands for the loss.
fn both_nuls(facts: &BannerFacts) -> bool {
    facts.placeholder_conflict && facts.nul_count > 0
}

fn placeholder(facts: &BannerFacts) -> Option<BannerSpec> {
    if !both_nuls(facts) {
        return None;
    }
    Some(spec(
        BannerKind::Placeholder,
        format!(
            "“{}” has both NUL bytes and ␀ characters. Both show as ␀ and are saved as ␀, so \
             the NUL bytes can't be saved as they were.",
            facts.name
        ),
        Vec::new(),
    ))
}

fn lossy(facts: &BannerFacts) -> Option<BannerSpec> {
    let loss = facts.loss?;
    if both_nuls(facts) {
        return None;
    }
    let encoding = &facts.encoding;
    let reason = match loss {
        Loss::Malformed => format!("some bytes aren't valid {encoding} and show as �"),
        Loss::NotRoundTrip => {
            format!("{encoding} doesn't write some of its characters back as the same bytes")
        }
    };
    Some(spec(
        BannerKind::Lossy,
        format!("This file can't be saved exactly as it was: {reason}."),
        vec![BannerAction::Reinterpret],
    ))
}

fn mixed_eol(facts: &BannerFacts) -> Option<BannerSpec> {
    if !facts.mixed_eol || facts.mixed_kept {
        return None;
    }
    let stats = facts.eol_stats;
    let counts: Vec<String> = [
        (stats.lf, Eol::Lf),
        (stats.crlf, Eol::CrLf),
        (stats.cr, Eol::Cr),
    ]
    .into_iter()
    .filter(|(count, _)| *count > 0)
    .map(|(count, eol)| format!("{} {}", grouped(count), eol.label()))
    .collect();
    Some(spec(
        BannerKind::MixedEol,
        format!(
            "Mixed line endings ({}): saving writes {} everywhere.",
            counts.join(", "),
            facts.eol.label()
        ),
        vec![
            BannerAction::Normalize(facts.eol),
            BannerAction::KeepMixedEndings,
        ],
    ))
}

fn long_lines(facts: &BannerFacts) -> Option<BannerSpec> {
    let name = &facts.name;
    let title = match facts.origin {
        TextOrigin::File => return None,
        TextOrigin::Formatted(kind) => format!(
            "“{name}” has very long lines and was opened formatted as {}: the text differs \
             from the file's bytes, and saving asks before writing it.",
            kind.label()
        ),
        TextOrigin::DisplayBreaks => format!(
            "“{name}” has very long lines, broken every {} characters for display. The text \
             isn't the file's, so it can't be saved.",
            grouped(DISPLAY_BREAK_CHARS)
        ),
    };
    Some(spec(BannerKind::LongLines, title, Vec::new()))
}

fn large_file(facts: &BannerFacts) -> Option<BannerSpec> {
    let size = facts.large_file?;
    Some(spec(
        BannerKind::LargeFile,
        format!(
            "Large file ({:.1} MiB): syntax highlighting, word wrap, whitespace and bracket \
             matching are off.",
            size as f64 / MIB as f64
        ),
        Vec::new(),
    ))
}

fn highlighting(facts: &BannerFacts) -> Option<BannerSpec> {
    let limits = facts.highlight_limited?;
    let bytes = limits.max_bytes as u64;
    let size = if bytes.is_multiple_of(MIB) {
        format!("{} MiB", bytes / MIB)
    } else {
        format!("{:.1} MiB", bytes as f64 / MIB as f64)
    };
    Some(spec(
        BannerKind::Highlighting,
        format!(
            "Syntax highlighting is off for files over {size} or {} lines, so that typing \
             stays fast.",
            grouped(limits.max_lines)
        ),
        vec![BannerAction::HighlightAnyway],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::pretty::FormatKind;

    fn facts() -> BannerFacts {
        BannerFacts {
            name: "notes.txt".to_owned(),
            path: Some(PathBuf::from("/tmp/notes.txt")),
            encoding: "UTF-8".to_owned(),
            ..BannerFacts::default()
        }
    }

    fn only(facts: &BannerFacts) -> BannerSpec {
        let mut banners = banners_for(facts);
        assert_eq!(banners.len(), 1, "{banners:#?}");
        banners.remove(0)
    }

    fn labels(spec: &BannerSpec) -> Vec<String> {
        spec.actions.iter().map(BannerAction::label).collect()
    }

    #[test]
    fn a_plain_document_has_no_banners() {
        assert!(banners_for(&facts()).is_empty());
    }

    #[test]
    fn a_file_changed_on_disk_offers_compare_reload_and_keep() {
        let spec = only(&BannerFacts {
            disk: DiskState::Changed,
            ..facts()
        });
        assert_eq!(spec.kind, BannerKind::Disk);
        assert_eq!(spec.title, "“notes.txt” changed on disk.");
        assert_eq!(labels(&spec), ["Compare", "Reload", "Keep Mine"]);
    }

    #[test]
    fn a_restored_backup_whose_file_changed_offers_the_disk_version_last() {
        let spec = only(&BannerFacts {
            disk: DiskState::Changed,
            restored_conflict: true,
            ..facts()
        });
        assert_eq!(
            spec.title,
            "The file “notes.txt” changed on disk since your unsaved changes."
        );
        assert_eq!(labels(&spec), ["Compare", "Keep Mine", "Load Disk Version"]);
    }

    #[test]
    fn a_deleted_file_offers_save() {
        let spec = only(&BannerFacts {
            disk: DiskState::Deleted {
                clean_revision: None,
            },
            ..facts()
        });
        assert_eq!(
            spec.title,
            "“notes.txt” is no longer on disk. Save it to keep it."
        );
        assert_eq!(spec.actions, [BannerAction::Save]);
    }

    #[test]
    fn a_file_this_user_may_not_write_gives_the_sudoedit_command() {
        let command = "SUDO_EDITOR=\"stet --wait\" sudoedit /tmp/notes.txt";
        let spec = only(&BannerFacts {
            read_only: Some(ReadOnly::NotWritable),
            unwritable: Some(Unwritable::Permission),
            ..facts()
        });
        assert_eq!(spec.kind, BannerKind::ReadOnly);
        assert_eq!(
            spec.title,
            format!(
                "You may not write “notes.txt”, so it is open read-only. To edit it as root, \
                 run: {command}"
            )
        );
        assert_eq!(
            spec.actions,
            [
                BannerAction::CopyCommand(command.to_owned()),
                BannerAction::EditAnyway
            ]
        );
        assert_eq!(labels(&spec), ["Copy Command", "Edit Anyway"]);

        let spec = only(&BannerFacts {
            unwritable: Some(Unwritable::Permission),
            ..facts()
        });
        assert_eq!(
            spec.title,
            format!(
                "You may not write “notes.txt”. Save a copy with Save As, or edit it as root: \
                 {command}"
            )
        );
        assert_eq!(labels(&spec), ["Copy Command"]);

        let spec = only(&BannerFacts {
            unwritable: Some(Unwritable::ReadOnlyFilesystem),
            ..facts()
        });
        assert_eq!(
            spec.title,
            "“notes.txt” is on a read-only file system. Save a copy with Save As."
        );
        assert!(spec.actions.is_empty());
    }

    #[test]
    fn a_binary_file_can_be_edited_only_when_it_round_trips() {
        let binary = BannerFacts {
            read_only: Some(ReadOnly::Binary),
            nul_count: 3,
            ..facts()
        };
        let spec = only(&binary);
        assert_eq!(spec.kind, BannerKind::Binary);
        assert_eq!(
            spec.title,
            "“notes.txt” looks binary (it has NUL bytes), so it is open read-only. NUL bytes \
             show as ␀ and are saved as NUL."
        );
        assert_eq!(spec.actions, [BannerAction::EditAnyway]);

        let mixed = banners_for(&BannerFacts {
            mixed_eol: true,
            mixed_kept: true,
            ..binary.clone()
        });
        assert_eq!(
            mixed[0].title,
            "“notes.txt” looks binary and can't be saved exactly as it was, so it is open \
             read-only."
        );
        assert!(mixed[0].actions.is_empty());

        let lossy = banners_for(&BannerFacts {
            loss: Some(Loss::NotRoundTrip),
            ..binary
        });
        let kinds: Vec<BannerKind> = lossy.iter().map(|spec| spec.kind).collect();
        assert_eq!(kinds, [BannerKind::Binary, BannerKind::Lossy]);
        assert!(lossy[0].actions.is_empty());
    }

    #[test]
    fn a_lossy_decode_says_why_and_offers_reinterpret() {
        let spec = only(&BannerFacts {
            loss: Some(Loss::Malformed),
            ..facts()
        });
        assert_eq!(spec.kind, BannerKind::Lossy);
        assert_eq!(
            spec.title,
            "This file can't be saved exactly as it was: some bytes aren't valid UTF-8 and \
             show as �."
        );
        assert_eq!(labels(&spec), ["Reinterpret…"]);

        let spec = only(&BannerFacts {
            loss: Some(Loss::NotRoundTrip),
            encoding: "Shift_JIS".to_owned(),
            ..facts()
        });
        assert_eq!(
            spec.title,
            "This file can't be saved exactly as it was: Shift_JIS doesn't write some of its \
             characters back as the same bytes."
        );
    }

    #[test]
    fn nul_bytes_beside_real_placeholders_replace_the_lossy_banner() {
        let spec = only(&BannerFacts {
            nul_count: 2,
            placeholder_conflict: true,
            loss: Some(Loss::NotRoundTrip),
            ..facts()
        });
        assert_eq!(spec.kind, BannerKind::Placeholder);
        assert_eq!(
            spec.title,
            "“notes.txt” has both NUL bytes and ␀ characters. Both show as ␀ and are saved as \
             ␀, so the NUL bytes can't be saved as they were."
        );
        assert!(spec.actions.is_empty());

        let placeholders_alone = BannerFacts {
            placeholder_conflict: true,
            ..facts()
        };
        assert!(banners_for(&placeholders_alone).is_empty());
    }

    #[test]
    fn mixed_line_endings_are_counted_until_kept_or_normalised() {
        let mixed = BannerFacts {
            mixed_eol: true,
            eol_stats: EolStats {
                lf: 1,
                crlf: 2_000,
                cr: 0,
            },
            eol: Eol::CrLf,
            ..facts()
        };
        let spec = only(&mixed);
        assert_eq!(spec.kind, BannerKind::MixedEol);
        assert_eq!(
            spec.title,
            "Mixed line endings (1 LF, 2,000 CRLF): saving writes CRLF everywhere."
        );
        assert_eq!(
            spec.actions,
            [
                BannerAction::Normalize(Eol::CrLf),
                BannerAction::KeepMixedEndings
            ]
        );
        assert_eq!(labels(&spec), ["Normalize to CRLF", "Keep"]);
        assert!(
            banners_for(&BannerFacts {
                mixed_kept: true,
                ..mixed
            })
            .is_empty()
        );
    }

    #[test]
    fn long_lines_say_how_the_text_differs_from_the_file() {
        let spec = only(&BannerFacts {
            origin: TextOrigin::Formatted(FormatKind::Json),
            ..facts()
        });
        assert_eq!(spec.kind, BannerKind::LongLines);
        assert_eq!(
            spec.title,
            "“notes.txt” has very long lines and was opened formatted as JSON: the text differs \
             from the file's bytes, and saving asks before writing it."
        );
        let spec = only(&BannerFacts {
            origin: TextOrigin::DisplayBreaks,
            ..facts()
        });
        assert_eq!(
            spec.title,
            format!(
                "“notes.txt” has very long lines, broken every {} characters for display. The \
                 text isn't the file's, so it can't be saved.",
                grouped(DISPLAY_BREAK_CHARS)
            )
        );
    }

    #[test]
    fn a_large_file_gives_its_size() {
        let spec = only(&BannerFacts {
            large_file: Some(100 * MIB),
            ..facts()
        });
        assert_eq!(spec.kind, BannerKind::LargeFile);
        assert_eq!(
            spec.title,
            "Large file (100.0 MiB): syntax highlighting, word wrap, whitespace and bracket \
             matching are off."
        );
    }

    #[test]
    fn highlighting_held_back_gives_the_limits() {
        let held = |max_bytes| BannerFacts {
            highlight_limited: Some(HighlightLimits {
                max_bytes,
                max_lines: 100_000,
            }),
            ..facts()
        };
        let spec = only(&held(2 * 1024 * 1024));
        assert_eq!(spec.kind, BannerKind::Highlighting);
        assert_eq!(
            spec.title,
            "Syntax highlighting is off for files over 2 MiB or 100,000 lines, so that typing \
             stays fast."
        );
        assert_eq!(labels(&spec), ["Highlight Anyway"]);
        assert!(only(&held(1_572_864)).title.contains("over 1.5 MiB or"));
    }

    #[test]
    fn banners_come_most_urgent_first() {
        let everything = BannerFacts {
            disk: DiskState::Changed,
            read_only: Some(ReadOnly::Binary),
            unwritable: Some(Unwritable::ReadOnlyFilesystem),
            loss: Some(Loss::Malformed),
            mixed_eol: true,
            eol_stats: EolStats {
                lf: 1,
                crlf: 1,
                cr: 1,
            },
            origin: TextOrigin::Formatted(FormatKind::Xml),
            large_file: Some(60 * MIB),
            highlight_limited: Some(HighlightLimits {
                max_bytes: 1024,
                max_lines: 10,
            }),
            ..facts()
        };
        let kinds: Vec<BannerKind> = banners_for(&everything)
            .iter()
            .map(|spec| spec.kind)
            .collect();
        assert_eq!(
            kinds,
            [
                BannerKind::Disk,
                BannerKind::ReadOnly,
                BannerKind::Binary,
                BannerKind::Lossy,
                BannerKind::MixedEol,
                BannerKind::LongLines,
                BannerKind::LargeFile,
                BannerKind::Highlighting,
            ]
        );
        let mut sorted = kinds.clone();
        sorted.sort();
        assert_eq!(kinds, sorted);
        assert!(
            kinds
                .iter()
                .all(|kind| BannerKind::FROM_STATE.contains(kind))
        );
    }
}
