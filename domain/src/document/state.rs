//! What a document's state says about its file: why the editor refuses edits, where the text
//! came from, why a save could not write the file, how the file on disk compares with the
//! document, and why saving can't give back the bytes that were read.

use crate::ops::pretty::FormatKind;

/// Why the editor refuses edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOnly {
    /// The file contains NUL bytes; "Edit Anyway" lifts it when the file round-trips.
    Binary,
    /// This user may not write the file; "Edit Anyway" lifts it, for Save As.
    NotWritable,
    /// Long lines were broken for display, so the text isn't the file's; saving is off.
    DisplayBreaks,
    /// A text a comparison opened (the clipboard, the saved version); it closes with the
    /// comparison.
    Comparison,
}

impl ReadOnly {
    /// Whether "Edit Anyway" may lift it: the text is the file's, only the file is protected.
    #[must_use]
    pub const fn can_lift(self) -> bool {
        matches!(self, Self::Binary | Self::NotWritable)
    }

    /// Why a save was refused before writing anything.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::Binary => {
                "This binary file is read-only; choose Edit Anyway on its banner to edit it."
            }
            Self::NotWritable => {
                "You may not write this file; save a copy with Save As, or edit it with sudoedit."
            }
            Self::DisplayBreaks => {
                "Long lines are broken for display, so this text isn't the file's and can't be saved."
            }
            Self::Comparison => {
                "This copy was opened for the comparison and can't be changed; Clear Compare closes it."
            }
        }
    }
}

/// Where the buffer's text came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextOrigin {
    /// Decoded from the file as it is, or typed into a new document.
    #[default]
    File,
    /// Pretty-printed by the long-line path: saving asks before writing it.
    Formatted(FormatKind),
    /// Long lines broken for display: read-only.
    DisplayBreaks,
}

/// Why the last save could not write the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unwritable {
    /// The user lacks permission to write the file or its folder.
    Permission,
    /// The file is on a file system mounted read-only.
    ReadOnlyFilesystem,
}

/// The file on disk, as last compared with the document's baseline fingerprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DiskState {
    /// The file matches the document's baseline.
    #[default]
    Same,
    /// Changed while the document had unsaved changes; the banner asks what to keep.
    Changed,
    /// Deleted or moved away. `clean_revision` is the buffer's revision at that moment when
    /// it had no unsaved changes, so a file that comes back can reload quietly.
    Deleted {
        /// The buffer's revision when the file went, if it had no unsaved changes then.
        clean_revision: Option<u64>,
    },
}

/// Why saving can't write back the bytes that were read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loss {
    /// Some bytes weren't valid in the encoding and show as U+FFFD.
    Malformed,
    /// The encoding doesn't write some of its characters back as the same bytes.
    NotRoundTrip,
}

impl Loss {
    /// What decoding reported: malformed bytes outweigh an encoding that doesn't round-trip.
    #[must_use]
    pub const fn of(malformed: bool, not_round_trip: bool) -> Option<Self> {
        if malformed {
            Some(Self::Malformed)
        } else if not_round_trip {
            Some(Self::NotRoundTrip)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_bytes_outweigh_a_lossy_encoding() {
        assert_eq!(Loss::of(true, true), Some(Loss::Malformed));
        assert_eq!(Loss::of(true, false), Some(Loss::Malformed));
        assert_eq!(Loss::of(false, true), Some(Loss::NotRoundTrip));
        assert_eq!(Loss::of(false, false), None);
    }

    #[test]
    fn only_a_protected_file_can_be_edited_anyway() {
        assert!(ReadOnly::Binary.can_lift());
        assert!(ReadOnly::NotWritable.can_lift());
        assert!(!ReadOnly::DisplayBreaks.can_lift());
        assert!(!ReadOnly::Comparison.can_lift());
    }
}
