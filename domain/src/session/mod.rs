//! The session that `session.json` stores, so tabs, unsaved text and window state survive
//! quitting, crashes and reboots (ADR-006). Pure data: `stet-infrastructure` reads and writes
//! it, and keeps each dirty or untitled tab's text in a backup file named by [`TabRecord::backup`].
//!
//! The format is forward-compatible:
//! - every field has a default, so a document with missing fields loads;
//! - unknown fields are ignored, so an older build reads a newer document and drops what it
//!   doesn't know the next time it writes;
//! - a value of the wrong type falls back to its default, per field of a session or tab and per
//!   list element, instead of failing the whole document.
//!
//! Only malformed JSON, or a `schema_version` that isn't a number, makes a document unreadable.
//! Paths that aren't UTF-8 are written as `{"bytes": [...]}`, line endings as `"LF"`, `"CRLF"`
//! or `"CR"`.

mod codec;
mod id;
#[cfg(test)]
mod tests;

use crate::text::{CharOffset, Eol, Line};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

pub use id::{InvalidId, MAX_ID_LEN, RecordId, is_valid_id};

/// The format version this build writes.
pub const SCHEMA_VERSION: u32 = 1;

/// Documents with more characters than this back off: at most one backup per
/// [`LARGE_BACKUP_INTERVAL`], and only once the text has been left alone for
/// [`LARGE_BACKUP_IDLE`]. A character takes at least a byte, so this is over 16 MiB.
pub const LARGE_BACKUP_CHARS: usize = 16 * 1024 * 1024;

pub const LARGE_BACKUP_INTERVAL: Duration = Duration::from_secs(60);

pub const LARGE_BACKUP_IDLE: Duration = Duration::from_secs(2);

/// Everything restored at startup: the tabs, the window and the histories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    /// The format version of the document; a document without one reads as
    /// [`SCHEMA_VERSION`]. A larger number means a newer build wrote it.
    pub schema_version: u32,
    /// Tabs in strip order. `--wait` tabs are never included.
    #[serde(deserialize_with = "codec::lenient_seq")]
    pub tabs: Vec<TabRecord>,
    /// Index into `tabs` of the tab to show first.
    #[serde(deserialize_with = "codec::lenient")]
    pub active_tab: Option<usize>,
    /// The main window's size and state.
    #[serde(deserialize_with = "codec::lenient")]
    pub window: WindowState,
    /// Most recent first.
    #[serde(with = "codec::path_seq")]
    pub recent_files: Vec<PathBuf>,
    /// Most recent first.
    #[serde(deserialize_with = "codec::lenient_seq")]
    pub find_history: Vec<String>,
    /// Most recent first.
    #[serde(deserialize_with = "codec::lenient_seq")]
    pub replace_history: Vec<String>,
    /// Find in Files' folders, most recent first.
    #[serde(deserialize_with = "codec::lenient_seq")]
    pub directory_history: Vec<String>,
    /// Find in Files' filters, most recent first.
    #[serde(deserialize_with = "codec::lenient_seq")]
    pub filter_history: Vec<String>,
    /// Index into `tabs` of the tab the other view shows, when the window is split.
    #[serde(deserialize_with = "codec::lenient")]
    pub other_tab: Option<usize>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            tabs: Vec::new(),
            active_tab: None,
            window: WindowState::default(),
            recent_files: Vec::new(),
            find_history: Vec::new(),
            replace_history: Vec::new(),
            directory_history: Vec::new(),
            filter_history: Vec::new(),
            other_tab: None,
        }
    }
}

impl Session {
    /// The backup ids the tabs reference.
    pub fn backup_ids(&self) -> impl Iterator<Item = &RecordId> {
        self.tabs.iter().filter_map(|tab| tab.backup.as_ref())
    }

    /// Gives tabs with the empty id (also what an invalid one reads as) or a duplicate id a
    /// fresh one from `new_id`, and clears an `active_tab` that is out of range. `new_id` must
    /// return ids not used elsewhere.
    pub fn repair(&mut self, mut new_id: impl FnMut() -> RecordId) {
        let mut seen = BTreeSet::new();
        for tab in &mut self.tabs {
            if tab.id.is_empty() || seen.contains(&tab.id) {
                tab.id = new_id();
            }
            seen.insert(tab.id.clone());
        }
        for tab in [&mut self.active_tab, &mut self.other_tab] {
            if tab.is_some_and(|index| index >= self.tabs.len()) {
                *tab = None;
            }
        }
    }

    /// This session without unsaved work, for "Forget unsaved drafts": untitled tabs are
    /// dropped, and file tabs lose their backup and dirty flag so they reopen from disk.
    #[must_use]
    pub fn without_drafts(&self) -> Self {
        let id_of = |index: Option<usize>| {
            index
                .and_then(|index| self.tabs.get(index))
                .map(|tab| tab.id.as_str())
        };
        let (active, other) = (id_of(self.active_tab), id_of(self.other_tab));
        let mut session = self.clone();
        let kept: BTreeSet<&str> = self
            .tabs
            .iter()
            .filter(|tab| tab.path.is_some())
            .map(|tab| tab.id.as_str())
            .collect();
        // A clone goes with the document it shows.
        session.tabs.retain(|tab| match &tab.clone_of {
            Some(original) => kept.contains(original.as_str()),
            None => tab.path.is_some(),
        });
        for tab in &mut session.tabs {
            tab.dirty = false;
            tab.backup = None;
        }
        let index_of =
            |id: Option<&str>| id.and_then(|id| session.tabs.iter().position(|tab| tab.id == id));
        session.active_tab = index_of(active);
        session.other_tab = index_of(other);
        session
    }
}

/// One tab. Offsets count characters, as `GtkTextIter` does (ADR-003). A crash between a
/// backup write and the manifest write can leave them past the end of the restored text, so
/// clamp them.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TabRecord {
    /// Stable identity across restarts; empty until [`Session::repair`] gives it one.
    #[serde(deserialize_with = "codec::lenient")]
    pub id: RecordId,
    /// The file, or `None` for an untitled tab.
    #[serde(with = "codec::opt_path")]
    pub path: Option<PathBuf>,
    /// The tab label, such as the file name or "new 3".
    #[serde(deserialize_with = "codec::lenient")]
    pub display_name: String,
    /// The name the user gave an untitled tab with Rename…, in place of its first line.
    #[serde(
        deserialize_with = "codec::lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub custom_name: Option<String>,
    /// The N of an untitled tab's "Untitled N"; see
    /// [`next_untitled_number`](crate::document::next_untitled_number).
    #[serde(deserialize_with = "codec::lenient")]
    pub untitled_number: Option<u32>,
    /// The buffer has unsaved changes.
    #[serde(deserialize_with = "codec::lenient")]
    pub dirty: bool,
    /// The id of the backup holding the buffer text. `None` restores the tab from `path`, or
    /// empty; so does the empty id, which names no backup, after it is reported as missing.
    #[serde(deserialize_with = "codec::lenient")]
    pub backup: Option<RecordId>,
    /// Opened read-only: a file Stet may not write, or `--read-only`.
    #[serde(deserialize_with = "codec::lenient")]
    pub read_only: bool,
    /// The encoding to save with, by its `encoding_rs` name.
    #[serde(deserialize_with = "codec::lenient")]
    pub encoding: Option<String>,
    /// Whether to write a byte-order mark.
    #[serde(deserialize_with = "codec::lenient")]
    pub bom: Option<bool>,
    /// The line ending to save with; the buffer itself holds LF (ADR-008).
    #[serde(with = "codec::opt_eol")]
    pub eol: Option<Eol>,
    /// The GtkSourceView language id; `None` detects it again.
    #[serde(deserialize_with = "codec::lenient")]
    pub language: Option<String>,
    /// The caret, in characters.
    #[serde(deserialize_with = "codec::lenient")]
    pub caret: CharOffset,
    /// Start and end of the selection, in characters; the caret is at one end.
    #[serde(deserialize_with = "codec::lenient")]
    pub selection: Option<(CharOffset, CharOffset)>,
    /// The topmost visible line, to restore the scroll position.
    #[serde(deserialize_with = "codec::lenient")]
    pub first_visible_line: Line,
    /// A pinned tab.
    #[serde(deserialize_with = "codec::lenient")]
    pub pinned: bool,
    /// The view the tab is in.
    #[serde(deserialize_with = "codec::lenient")]
    pub view: ViewSlot,
    /// A clone: the id of the tab whose document this tab shows too. Its record keeps its
    /// own place in its view (caret, selection, first visible line, pinned); the document's
    /// state is the other record's.
    #[serde(deserialize_with = "codec::lenient")]
    pub clone_of: Option<RecordId>,
    /// Bookmarked lines.
    #[serde(deserialize_with = "codec::lenient_seq")]
    pub bookmarks: Vec<Line>,
    /// The file as it was when the buffer was last loaded from or saved to it. Restore compares
    /// it with the file now to notice changes made while Stet was closed.
    #[serde(deserialize_with = "codec::lenient")]
    pub disk_fingerprint: Option<Fingerprint>,
    /// An encoding the user picked with Reinterpret, by its encoding-table id (such as
    /// `windows-1252` or `UTF-8+bom`): the file is read in it again instead of detecting one.
    #[serde(deserialize_with = "codec::lenient")]
    pub chosen_encoding: Option<String>,
    /// Saving can't reproduce the file's bytes (decoding errors or a lossy encoding), so a
    /// save of the restored text still asks first.
    #[serde(deserialize_with = "codec::lenient")]
    pub lossy: bool,
}

impl TabRecord {
    /// The selection as (anchor, caret), clamped to `len` characters: the caret is at the
    /// end of `selection` it equals, so a selection made backwards stays backwards. Without a
    /// selection, both are the caret.
    pub fn anchor_and_caret(&self, len: usize) -> (CharOffset, CharOffset) {
        let end_of_text = CharOffset::ZERO + len;
        let caret = self.caret.min(end_of_text);
        match self.selection {
            Some((start, end)) => {
                let (start, end) = (
                    start.min(end).min(end_of_text),
                    start.max(end).min(end_of_text),
                );
                if caret == start {
                    (end, start)
                } else {
                    (start, end)
                }
            }
            None => (caret, caret),
        }
    }
}

/// Which view of a window a tab is in. `session.json` writes it as a number: 0 for the main
/// view, 1 for the second view of a split window. A larger number, which no build writes, reads
/// as the second view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(from = "u8", into = "u8")]
pub enum ViewSlot {
    /// The main view, the only one of a window that isn't split.
    #[default]
    Main,
    /// The second view of a split window.
    Second,
}

impl ViewSlot {
    /// The slot at `index` among a window's views: 0 is the main view, any other the second.
    pub const fn from_index(index: usize) -> Self {
        if index == 0 { Self::Main } else { Self::Second }
    }

    /// The index of the slot among a window's views.
    pub const fn index(self) -> usize {
        match self {
            Self::Main => 0,
            Self::Second => 1,
        }
    }
}

impl From<u8> for ViewSlot {
    fn from(number: u8) -> Self {
        Self::from_index(usize::from(number))
    }
}

impl From<ViewSlot> for u8 {
    fn from(slot: ViewSlot) -> Self {
        match slot {
            ViewSlot::Main => 0,
            ViewSlot::Second => 1,
        }
    }
}

/// The main window. Sizes are in logical pixels; 0 means unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowState {
    pub width: i32,
    pub height: i32,
    pub maximized: bool,
    /// The editor zoom in points over the base font size, as `stet_domain::view::Zoom` has it.
    pub zoom: i32,
    /// Where the divider between the two views was, in logical pixels from the left; 0
    /// means in the middle.
    pub split_position: i32,
}

/// What a document's backup needs at a commit (ADR-006).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackupState {
    /// The text has to be kept: unsaved changes, or an untitled document with text in it.
    pub needs_backup: bool,
    /// The text changed since its last backup, or it has none yet.
    pub changed: bool,
    /// In large-file mode: no periodic backup, and closing the window asks instead.
    pub large_file: bool,
    pub chars: usize,
    /// Since the last backup of this document was written, if it has one.
    pub since_backup: Option<Duration>,
    /// Since the text last changed.
    pub since_edit: Duration,
}

/// What to do with a document's backup at a commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupAction {
    /// Snapshot the text and write it.
    Write,
    /// Keep the backup the document has (if any) as it is.
    Keep,
    /// The text needs no backup: delete the one it has.
    Remove,
}

impl BackupState {
    /// The policy: every commit backs up a text that changed, except that large-file mode
    /// never does periodically, and documents over [`LARGE_BACKUP_CHARS`] wait for
    /// [`LARGE_BACKUP_INTERVAL`] since their last backup and [`LARGE_BACKUP_IDLE`] since the
    /// last edit. The last commit before quitting (`last`) writes every changed text.
    pub fn action(&self, last: bool) -> BackupAction {
        if !self.needs_backup {
            return BackupAction::Remove;
        }
        if !self.changed {
            return BackupAction::Keep;
        }
        if last {
            return BackupAction::Write;
        }
        if self.large_file {
            return BackupAction::Keep;
        }
        if self.chars > LARGE_BACKUP_CHARS {
            let due = self
                .since_backup
                .is_none_or(|elapsed| elapsed >= LARGE_BACKUP_INTERVAL);
            if !due || self.since_edit < LARGE_BACKUP_IDLE {
                return BackupAction::Keep;
            }
        }
        BackupAction::Write
    }
}

/// Identity and version of a file: device, inode, size, and modification time in nanoseconds
/// since the Unix epoch. All four fields are required; a partial fingerprint reads as none.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Fingerprint {
    pub dev: u64,
    pub ino: u64,
    pub size: u64,
    pub mtime_ns: i64,
}

impl Fingerprint {
    /// Whether `now` shows the same file, unchanged. `dev` is not compared: btrfs subvolumes
    /// and device-mapper volumes get their device numbers when mounted or activated, so they
    /// can differ after a reboot.
    pub fn matches(&self, now: &Self) -> bool {
        self.ino == now.ino && self.size == now.size && self.mtime_ns == now.mtime_ns
    }
}
