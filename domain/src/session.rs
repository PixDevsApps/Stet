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

use crate::text::Eol;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

/// The format version this build writes.
pub const SCHEMA_VERSION: u32 = 1;

/// The longest tab or backup id, in bytes.
pub const MAX_ID_LEN: usize = 64;

/// How often the unsaved text of dirty and untitled documents is backed up (ADR-006).
pub const BACKUP_INTERVAL: Duration = Duration::from_secs(7);

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
    /// Index into `tabs` of the tab the other view shows, when the window is split (M7).
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
    pub fn backup_ids(&self) -> impl Iterator<Item = &str> {
        self.tabs.iter().filter_map(|tab| tab.backup.as_deref())
    }

    /// The N for the next untitled tab's "Untitled N": the lowest number no untitled tab uses,
    /// so freed numbers are reused.
    pub fn next_untitled_number(&self) -> u32 {
        next_untitled_number(
            self.tabs
                .iter()
                .filter(|tab| tab.path.is_none())
                .filter_map(|tab| tab.untitled_number),
        )
    }

    /// Gives tabs with an invalid or duplicate id a fresh one from `new_id`, and clears an
    /// `active_tab` that is out of range. `new_id` must return valid ids not used elsewhere.
    pub fn repair(&mut self, mut new_id: impl FnMut() -> String) {
        let mut seen = BTreeSet::new();
        for tab in &mut self.tabs {
            if !is_valid_id(&tab.id) || seen.contains(&tab.id) {
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
    /// Stable identity across restarts; see [`is_valid_id`].
    #[serde(deserialize_with = "codec::lenient")]
    pub id: String,
    /// The file, or `None` for an untitled tab.
    #[serde(with = "codec::opt_path")]
    pub path: Option<PathBuf>,
    /// The tab label, such as the file name or "new 3".
    #[serde(deserialize_with = "codec::lenient")]
    pub display_name: String,
    /// The name the user gave an untitled tab with Rename… (1.2), in place of its first line.
    #[serde(
        deserialize_with = "codec::lenient",
        skip_serializing_if = "Option::is_none"
    )]
    pub custom_name: Option<String>,
    /// The N of an untitled tab's "new N"; see [`Session::next_untitled_number`].
    #[serde(deserialize_with = "codec::lenient")]
    pub untitled_number: Option<u32>,
    /// The buffer has unsaved changes.
    #[serde(deserialize_with = "codec::lenient")]
    pub dirty: bool,
    /// The id of the backup holding the buffer text (see [`is_valid_id`]). `None` restores
    /// the tab from `path`, or empty.
    #[serde(deserialize_with = "codec::lenient")]
    pub backup: Option<String>,
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
    pub caret: usize,
    /// Start and end of the selection, in characters; the caret is at one end.
    #[serde(deserialize_with = "codec::lenient")]
    pub selection: Option<(usize, usize)>,
    /// The topmost visible line, zero-based, to restore the scroll position.
    #[serde(deserialize_with = "codec::lenient")]
    pub first_visible_line: usize,
    /// A pinned tab (M8).
    #[serde(deserialize_with = "codec::lenient")]
    pub pinned: bool,
    /// The view the tab is in (M7): 0 is the main view, 1 the second view of a split window.
    #[serde(deserialize_with = "codec::lenient")]
    pub view: u8,
    /// A clone (M7): the id of the tab whose document this tab shows too. Its record keeps its
    /// own place in its view (caret, selection, first visible line, pinned); the document's
    /// state is the other record's.
    #[serde(deserialize_with = "codec::lenient")]
    pub clone_of: Option<String>,
    /// Bookmarked lines, zero-based (M7).
    #[serde(deserialize_with = "codec::lenient_seq")]
    pub bookmarks: Vec<usize>,
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
    pub fn anchor_and_caret(&self, len: usize) -> (usize, usize) {
        let caret = self.caret.min(len);
        match self.selection {
            Some((start, end)) => {
                let (start, end) = (start.min(end).min(len), start.max(end).min(len));
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

/// The main window. Sizes are in logical pixels; 0 means unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowState {
    pub width: i32,
    pub height: i32,
    pub maximized: bool,
    /// The editor zoom in points over the base font size, as `stet_domain::view::Zoom` has it.
    pub zoom: i32,
    /// Where the divider between the two views was (M7), in logical pixels from the left; 0
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

/// Whether `id` can name a tab or backup: 1 to [`MAX_ID_LEN`] ASCII letters, digits, `-` and
/// `_`, so it is always a plain file name.
pub fn is_valid_id(id: &str) -> bool {
    (1..=MAX_ID_LEN).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

/// The lowest number from 1 up that isn't in `used`.
pub fn next_untitled_number(used: impl IntoIterator<Item = u32>) -> u32 {
    let used: BTreeSet<u32> = used.into_iter().collect();
    (1..=u32::MAX)
        .find(|number| !used.contains(number))
        .unwrap_or(u32::MAX)
}

/// Serde helpers for the lenient, forward-compatible format.
mod codec {
    use crate::text::Eol;
    use serde::de::IgnoredAny;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::borrow::Cow;
    use std::ffi::OsString;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::path::{Path, PathBuf};

    /// A value of the expected shape, or anything else.
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Lenient<T> {
        Valid(T),
        Invalid(IgnoredAny),
    }

    impl<T> Lenient<T> {
        fn valid(self) -> Option<T> {
            match self {
                Self::Valid(value) => Some(value),
                Self::Invalid(_) => None,
            }
        }
    }

    /// Deserializes `T`, or gives its default when the value has another shape.
    pub fn lenient<'de, D, T>(deserializer: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de> + Default,
    {
        Ok(Lenient::deserialize(deserializer)?
            .valid()
            .unwrap_or_default())
    }

    /// Deserializes a list, skipping elements of another shape; anything but a list is empty.
    pub fn lenient_seq<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        let items: Vec<Lenient<T>> = lenient(deserializer)?;
        Ok(items.into_iter().filter_map(Lenient::valid).collect())
    }

    /// A path as text, or as raw bytes when it isn't UTF-8.
    #[derive(Serialize, Deserialize)]
    #[serde(untagged)]
    enum PathRepr<'a> {
        Text(Cow<'a, str>),
        Bytes { bytes: Cow<'a, [u8]> },
    }

    impl<'a> From<&'a Path> for PathRepr<'a> {
        fn from(path: &'a Path) -> Self {
            match path.to_str() {
                Some(text) => Self::Text(Cow::Borrowed(text)),
                None => Self::Bytes {
                    bytes: Cow::Borrowed(path.as_os_str().as_bytes()),
                },
            }
        }
    }

    impl From<PathRepr<'_>> for PathBuf {
        fn from(repr: PathRepr<'_>) -> Self {
            match repr {
                PathRepr::Text(text) => PathBuf::from(text.into_owned()),
                PathRepr::Bytes { bytes } => PathBuf::from(OsString::from_vec(bytes.into_owned())),
            }
        }
    }

    pub mod opt_path {
        use super::*;

        pub fn serialize<S: Serializer>(
            path: &Option<PathBuf>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            path.as_deref().map(PathRepr::from).serialize(serializer)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<PathBuf>, D::Error> {
            let repr: Option<PathRepr<'_>> = lenient(deserializer)?;
            Ok(repr.map(PathBuf::from))
        }
    }

    pub mod path_seq {
        use super::*;

        pub fn serialize<S: Serializer>(
            paths: &[PathBuf],
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            serializer.collect_seq(paths.iter().map(|path| PathRepr::from(path.as_path())))
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Vec<PathBuf>, D::Error> {
            let reprs: Vec<PathRepr<'_>> = lenient_seq(deserializer)?;
            Ok(reprs.into_iter().map(PathBuf::from).collect())
        }
    }

    pub mod opt_eol {
        use super::*;

        pub fn serialize<S: Serializer>(
            eol: &Option<Eol>,
            serializer: S,
        ) -> Result<S::Ok, S::Error> {
            eol.map(Eol::label).serialize(serializer)
        }

        /// An unknown label reads as `None`.
        pub fn deserialize<'de, D: Deserializer<'de>>(
            deserializer: D,
        ) -> Result<Option<Eol>, D::Error> {
            let label: Option<String> = lenient(deserializer)?;
            Ok(label.and_then(|label| Eol::ALL.into_iter().find(|eol| eol.label() == label)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::collection::vec;
    use proptest::prelude::*;
    use serde_json::{Value, json};
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    fn non_utf8_path() -> PathBuf {
        PathBuf::from(OsString::from_vec(b"/tmp/caf\xe9.txt".to_vec()))
    }

    fn sample() -> Session {
        Session {
            tabs: vec![
                TabRecord {
                    id: "0199a3b2-7c1e-7d4a-9f00-3c5e8b2a1d10".into(),
                    path: Some("/home/user/notes/todo.md".into()),
                    display_name: "todo.md".into(),
                    dirty: true,
                    backup: Some("0199a3b2-7c1e-7d4a-9f00-3c5e8b2a1d10".into()),
                    encoding: Some("UTF-8".into()),
                    bom: Some(false),
                    eol: Some(Eol::CrLf),
                    language: Some("markdown".into()),
                    caret: 120,
                    selection: Some((100, 120)),
                    first_visible_line: 3,
                    disk_fingerprint: Some(Fingerprint {
                        dev: 57,
                        ino: 10339,
                        size: 2048,
                        mtime_ns: 1_790_000_000_123_456_789,
                    }),
                    chosen_encoding: Some("windows-1252".into()),
                    lossy: true,
                    bookmarks: vec![3, 10],
                    ..TabRecord::default()
                },
                TabRecord {
                    id: "0199a3b2-7c1f-7e11-8a42-51c0d9e6f7a2".into(),
                    display_name: "Shopping list".into(),
                    custom_name: Some("Shopping list".into()),
                    untitled_number: Some(1),
                    dirty: true,
                    backup: Some("0199a3b2-7c1f-7e11-8a42-51c0d9e6f7a2".into()),
                    pinned: true,
                    view: 1,
                    ..TabRecord::default()
                },
                TabRecord {
                    id: "0199a3b2-7c20-7f00-9b11-0a1b2c3d4e5f".into(),
                    display_name: "todo.md".into(),
                    caret: 4,
                    view: 1,
                    clone_of: Some("0199a3b2-7c1e-7d4a-9f00-3c5e8b2a1d10".into()),
                    ..TabRecord::default()
                },
            ],
            active_tab: Some(1),
            other_tab: Some(0),
            window: WindowState {
                width: 1400,
                height: 900,
                maximized: false,
                zoom: 2,
                split_position: 700,
            },
            recent_files: vec!["/home/user/notes/todo.md".into(), non_utf8_path()],
            find_history: vec!["TODO".into()],
            replace_history: vec!["DONE".into()],
            directory_history: vec!["~/src".into()],
            filter_history: vec!["*.rs !target/".into()],
            ..Session::default()
        }
    }

    fn tab_with_id(id: &str) -> TabRecord {
        TabRecord {
            id: id.into(),
            ..TabRecord::default()
        }
    }

    fn ids(session: &Session) -> Vec<&str> {
        session.tabs.iter().map(|tab| tab.id.as_str()).collect()
    }

    #[test]
    fn json_format() {
        insta::assert_snapshot!(serde_json::to_string_pretty(&sample()).unwrap(), @r#"
        {
          "schema_version": 1,
          "tabs": [
            {
              "id": "0199a3b2-7c1e-7d4a-9f00-3c5e8b2a1d10",
              "path": "/home/user/notes/todo.md",
              "display_name": "todo.md",
              "untitled_number": null,
              "dirty": true,
              "backup": "0199a3b2-7c1e-7d4a-9f00-3c5e8b2a1d10",
              "read_only": false,
              "encoding": "UTF-8",
              "bom": false,
              "eol": "CRLF",
              "language": "markdown",
              "caret": 120,
              "selection": [
                100,
                120
              ],
              "first_visible_line": 3,
              "pinned": false,
              "view": 0,
              "clone_of": null,
              "bookmarks": [
                3,
                10
              ],
              "disk_fingerprint": {
                "dev": 57,
                "ino": 10339,
                "size": 2048,
                "mtime_ns": 1790000000123456789
              },
              "chosen_encoding": "windows-1252",
              "lossy": true
            },
            {
              "id": "0199a3b2-7c1f-7e11-8a42-51c0d9e6f7a2",
              "path": null,
              "display_name": "Shopping list",
              "custom_name": "Shopping list",
              "untitled_number": 1,
              "dirty": true,
              "backup": "0199a3b2-7c1f-7e11-8a42-51c0d9e6f7a2",
              "read_only": false,
              "encoding": null,
              "bom": null,
              "eol": null,
              "language": null,
              "caret": 0,
              "selection": null,
              "first_visible_line": 0,
              "pinned": true,
              "view": 1,
              "clone_of": null,
              "bookmarks": [],
              "disk_fingerprint": null,
              "chosen_encoding": null,
              "lossy": false
            },
            {
              "id": "0199a3b2-7c20-7f00-9b11-0a1b2c3d4e5f",
              "path": null,
              "display_name": "todo.md",
              "untitled_number": null,
              "dirty": false,
              "backup": null,
              "read_only": false,
              "encoding": null,
              "bom": null,
              "eol": null,
              "language": null,
              "caret": 4,
              "selection": null,
              "first_visible_line": 0,
              "pinned": false,
              "view": 1,
              "clone_of": "0199a3b2-7c1e-7d4a-9f00-3c5e8b2a1d10",
              "bookmarks": [],
              "disk_fingerprint": null,
              "chosen_encoding": null,
              "lossy": false
            }
          ],
          "active_tab": 1,
          "window": {
            "width": 1400,
            "height": 900,
            "maximized": false,
            "zoom": 2,
            "split_position": 700
          },
          "recent_files": [
            "/home/user/notes/todo.md",
            {
              "bytes": [
                47,
                116,
                109,
                112,
                47,
                99,
                97,
                102,
                233,
                46,
                116,
                120,
                116
              ]
            }
          ],
          "find_history": [
            "TODO"
          ],
          "replace_history": [
            "DONE"
          ],
          "directory_history": [
            "~/src"
          ],
          "filter_history": [
            "*.rs !target/"
          ],
          "other_tab": 0
        }
        "#);
    }

    #[test]
    fn an_empty_object_is_the_default_session() {
        let session: Session = serde_json::from_str("{}").unwrap();
        assert_eq!(session, Session::default());
        assert_eq!(session.schema_version, SCHEMA_VERSION);
    }

    #[test]
    fn rejects_malformed_documents() {
        for document in ["null", "\"session\"", r#"{"schema_version": "1"}"#, "{"] {
            assert!(
                serde_json::from_str::<Session>(document).is_err(),
                "{document}"
            );
        }
    }

    #[test]
    fn a_newer_version_still_loads() {
        let session: Session = serde_json::from_value(json!({
            "schema_version": 7,
            "tabs": [{"id": "a", "future_field": {"x": 1}}],
            "future_section": [1, 2, 3],
        }))
        .unwrap();
        assert_eq!(session.schema_version, 7);
        assert_eq!(ids(&session), ["a"]);
    }

    #[test]
    fn a_wrong_type_falls_back_per_field() {
        let original = serde_json::to_value(sample()).unwrap();
        let invalid = json!({"unexpected": [1, "x"]});
        let tab_defaults = serde_json::to_value(TabRecord::default()).unwrap();
        for key in original["tabs"][0].as_object().unwrap().keys() {
            let mut broken = original.clone();
            broken["tabs"][0][key] = invalid.clone();
            let mut expected = original.clone();
            expected["tabs"][0][key] = tab_defaults[key].clone();
            assert_eq!(
                serde_json::from_value::<Session>(broken).unwrap(),
                serde_json::from_value::<Session>(expected).unwrap(),
                "tab field {key}"
            );
        }
        let session_defaults = serde_json::to_value(Session::default()).unwrap();
        for key in original.as_object().unwrap().keys() {
            if key == "schema_version" {
                continue;
            }
            let mut broken = original.clone();
            broken[key] = invalid.clone();
            let mut expected = original.clone();
            expected[key] = session_defaults[key].clone();
            assert_eq!(
                serde_json::from_value::<Session>(broken).unwrap(),
                serde_json::from_value::<Session>(expected).unwrap(),
                "session field {key}"
            );
        }
    }

    #[test]
    fn list_elements_of_another_shape_are_skipped() {
        let session: Session = serde_json::from_value(json!({
            "tabs": [{"id": "a"}, 42, "tab", {"id": "b"}],
            "recent_files": ["/a", 7, {"bytes": [47, 98]}, null],
            "find_history": ["x", 1, "y"],
            "replace_history": "not a list",
        }))
        .unwrap();
        assert_eq!(ids(&session), ["a", "b"]);
        assert_eq!(
            session.recent_files,
            [PathBuf::from("/a"), PathBuf::from("/b")]
        );
        assert_eq!(session.find_history, ["x", "y"]);
        assert!(session.replace_history.is_empty());
    }

    #[test]
    fn line_endings_are_written_as_labels() {
        for eol in Eol::ALL {
            let value = serde_json::to_value(TabRecord {
                eol: Some(eol),
                ..TabRecord::default()
            })
            .unwrap();
            assert_eq!(value["eol"], eol.label());
            let tab: TabRecord = serde_json::from_value(value).unwrap();
            assert_eq!(tab.eol, Some(eol));
        }
        let tab: TabRecord = serde_json::from_value(json!({"eol": "NEL"})).unwrap();
        assert_eq!(tab.eol, None);
    }

    #[test]
    fn non_utf8_paths_are_written_as_bytes() {
        let value = serde_json::to_value(TabRecord {
            path: Some(non_utf8_path()),
            ..TabRecord::default()
        })
        .unwrap();
        assert_eq!(
            value["path"],
            json!({"bytes": b"/tmp/caf\xe9.txt".to_vec()})
        );
        let tab: TabRecord = serde_json::from_value(value).unwrap();
        assert_eq!(tab.path, Some(non_utf8_path()));
    }

    #[test]
    fn a_partial_fingerprint_reads_as_none() {
        let tab: TabRecord = serde_json::from_value(json!({
            "disk_fingerprint": {"dev": 1, "ino": 2, "size": 3},
        }))
        .unwrap();
        assert_eq!(tab.disk_fingerprint, None);
    }

    #[test]
    fn fingerprints_ignore_the_device_number() {
        let saved = Fingerprint {
            dev: 57,
            ino: 10,
            size: 5,
            mtime_ns: 1,
        };
        assert!(saved.matches(&Fingerprint { dev: 58, ..saved }));
        assert!(!saved.matches(&Fingerprint { ino: 11, ..saved }));
        assert!(!saved.matches(&Fingerprint { size: 6, ..saved }));
        assert!(!saved.matches(&Fingerprint {
            mtime_ns: 2,
            ..saved
        }));
    }

    #[test]
    fn ids_are_plain_file_names() {
        assert!(is_valid_id("0199a3b2-7c1e-7d4a-9f00-3c5e8b2a1d10"));
        assert!(is_valid_id("tab_1"));
        assert!(is_valid_id(&"x".repeat(MAX_ID_LEN)));
        let too_long = "x".repeat(MAX_ID_LEN + 1);
        for id in ["", ".", "..", "a/b", "../x", "a.txt", "a b", "é", &too_long] {
            assert!(!is_valid_id(id), "{id}");
        }
    }

    #[test]
    fn untitled_numbers_reuse_the_lowest_free_one() {
        assert_eq!(next_untitled_number([]), 1);
        assert_eq!(next_untitled_number([1, 2, 3]), 4);
        assert_eq!(next_untitled_number([2, 3]), 1);
        assert_eq!(next_untitled_number([0, 1, 3, 3]), 2);

        let untitled = |number| TabRecord {
            untitled_number: Some(number),
            ..TabRecord::default()
        };
        let saved = TabRecord {
            path: Some("/saved".into()),
            ..untitled(3)
        };
        let session = Session {
            tabs: vec![untitled(1), untitled(2), saved],
            ..Session::default()
        };
        assert_eq!(session.next_untitled_number(), 3);
    }

    #[test]
    fn repair_replaces_invalid_and_duplicate_ids() {
        let mut session = Session {
            tabs: ["a", "", "a", "../x", "b"].map(tab_with_id).to_vec(),
            active_tab: Some(5),
            ..Session::default()
        };
        let mut counter = 0;
        session.repair(|| {
            counter += 1;
            format!("new-{counter}")
        });
        assert_eq!(ids(&session), ["a", "new-1", "new-2", "new-3", "b"]);
        assert_eq!(session.active_tab, None);

        session.other_tab = Some(9);
        session.repair(|| unreachable!("all ids are valid and unique"));
        assert_eq!(session.other_tab, None);

        session.active_tab = Some(4);
        session.repair(|| unreachable!("all ids are valid and unique"));
        assert_eq!(session.active_tab, Some(4));
    }

    #[test]
    fn forgetting_drafts_keeps_file_tabs_clean() {
        let mut session = sample();
        session.tabs.push(TabRecord {
            path: Some("/c".into()),
            ..tab_with_id("c")
        });
        session.tabs.push(TabRecord {
            clone_of: Some(session.tabs[1].id.clone()),
            ..tab_with_id("untitled-clone")
        });
        session.active_tab = Some(3);
        session.other_tab = Some(2);
        let forgotten = session.without_drafts();
        // The untitled tab goes, and its clone with it; the file's clone stays.
        assert_eq!(
            ids(&forgotten),
            [
                session.tabs[0].id.as_str(),
                session.tabs[2].id.as_str(),
                "c"
            ]
        );
        assert!(
            forgotten
                .tabs
                .iter()
                .all(|tab| !tab.dirty && tab.backup.is_none())
        );
        assert_eq!(forgotten.tabs[0].caret, session.tabs[0].caret);
        assert_eq!(forgotten.active_tab, Some(2));
        assert_eq!(forgotten.other_tab, Some(1));
        assert_eq!(forgotten.recent_files, session.recent_files);
        assert_eq!(forgotten.find_history, session.find_history);

        assert_eq!(sample().without_drafts().active_tab, None);
    }

    #[test]
    fn selections_keep_their_direction() {
        let tab = |caret, selection| TabRecord {
            caret,
            selection,
            ..TabRecord::default()
        };
        assert_eq!(tab(5, None).anchor_and_caret(10), (5, 5));
        assert_eq!(tab(50, None).anchor_and_caret(10), (10, 10));
        assert_eq!(tab(8, Some((2, 8))).anchor_and_caret(10), (2, 8));
        assert_eq!(tab(2, Some((2, 8))).anchor_and_caret(10), (8, 2));
        assert_eq!(tab(2, Some((8, 2))).anchor_and_caret(10), (8, 2));
        // Past the end of a text that a crash left shorter.
        assert_eq!(tab(30, Some((20, 30))).anchor_and_caret(25), (20, 25));
        assert_eq!(tab(20, Some((20, 30))).anchor_and_caret(15), (15, 15));
    }

    fn backup(chars: usize) -> BackupState {
        BackupState {
            needs_backup: true,
            changed: true,
            large_file: false,
            chars,
            since_backup: None,
            since_edit: Duration::ZERO,
        }
    }

    #[test]
    fn changed_texts_are_backed_up_and_clean_ones_removed() {
        assert_eq!(backup(10).action(false), BackupAction::Write);
        let unchanged = BackupState {
            changed: false,
            ..backup(10)
        };
        assert_eq!(unchanged.action(false), BackupAction::Keep);
        assert_eq!(unchanged.action(true), BackupAction::Keep);
        let saved = BackupState {
            needs_backup: false,
            ..backup(10)
        };
        assert_eq!(saved.action(false), BackupAction::Remove);
        assert_eq!(saved.action(true), BackupAction::Remove);
    }

    #[test]
    fn large_texts_back_off_and_large_files_wait_for_the_last_commit() {
        let large = LARGE_BACKUP_CHARS + 1;
        assert_eq!(
            backup(LARGE_BACKUP_CHARS).action(false),
            BackupAction::Write
        );
        // Being typed in: wait until the text is left alone.
        assert_eq!(backup(large).action(false), BackupAction::Keep);
        let idle = BackupState {
            since_edit: LARGE_BACKUP_IDLE,
            ..backup(large)
        };
        assert_eq!(idle.action(false), BackupAction::Write);
        let recent = BackupState {
            since_backup: Some(LARGE_BACKUP_INTERVAL - Duration::from_secs(1)),
            ..idle
        };
        assert_eq!(recent.action(false), BackupAction::Keep);
        let due = BackupState {
            since_backup: Some(LARGE_BACKUP_INTERVAL),
            ..idle
        };
        assert_eq!(due.action(false), BackupAction::Write);
        assert_eq!(recent.action(true), BackupAction::Write);

        let large_file = BackupState {
            large_file: true,
            since_edit: Duration::from_secs(600),
            ..backup(10)
        };
        assert_eq!(large_file.action(false), BackupAction::Keep);
        assert_eq!(large_file.action(true), BackupAction::Write);
    }

    fn text() -> impl Strategy<Value = String> {
        ".{0,12}"
    }

    fn path() -> impl Strategy<Value = PathBuf> {
        prop_oneof![
            "(/[a-zé漢 ._-]{0,8}){1,3}".prop_map(PathBuf::from),
            vec(any::<u8>(), 0..16).prop_map(|bytes| PathBuf::from(OsString::from_vec(bytes))),
        ]
    }

    fn fingerprint() -> impl Strategy<Value = Fingerprint> {
        (any::<u64>(), any::<u64>(), any::<u64>(), any::<i64>()).prop_map(
            |(dev, ino, size, mtime_ns)| Fingerprint {
                dev,
                ino,
                size,
                mtime_ns,
            },
        )
    }

    fn tab() -> impl Strategy<Value = TabRecord> {
        let identity = (
            text(),
            proptest::option::of(path()),
            text(),
            any::<Option<u32>>(),
            any::<bool>(),
            proptest::option::of(text()),
            any::<bool>(),
            proptest::option::of(text()),
        );
        let view = (
            any::<Option<bool>>(),
            proptest::option::of(proptest::sample::select(Eol::ALL.to_vec())),
            proptest::option::of(text()),
            any::<usize>(),
            any::<Option<(usize, usize)>>(),
            any::<usize>(),
            any::<bool>(),
            proptest::option::of(fingerprint()),
        );
        let reading = (
            proptest::option::of(text()),
            any::<bool>(),
            proptest::option::of(text()),
        );
        let views = (
            any::<u8>(),
            proptest::option::of(text()),
            vec(any::<usize>(), 0..4),
        );
        (identity, view, reading, views).prop_map(
            |(
                (id, path, display_name, untitled_number, dirty, backup, read_only, encoding),
                (
                    bom,
                    eol,
                    language,
                    caret,
                    selection,
                    first_visible_line,
                    pinned,
                    disk_fingerprint,
                ),
                (chosen_encoding, lossy, custom_name),
                (view, clone_of, bookmarks),
            )| TabRecord {
                id,
                path,
                display_name,
                custom_name,
                untitled_number,
                dirty,
                backup,
                read_only,
                encoding,
                bom,
                eol,
                language,
                caret,
                selection,
                first_visible_line,
                pinned,
                disk_fingerprint,
                chosen_encoding,
                lossy,
                view,
                clone_of,
                bookmarks,
            },
        )
    }

    fn session() -> impl Strategy<Value = Session> {
        (
            any::<u32>(),
            vec(tab(), 0..6),
            any::<(Option<usize>, Option<usize>)>(),
            any::<(i32, i32, bool, i32, i32)>(),
            vec(path(), 0..4),
            vec(text(), 0..4),
            vec(text(), 0..4),
            vec(text(), 0..4),
            vec(text(), 0..4),
        )
            .prop_map(
                |(
                    schema_version,
                    tabs,
                    (active_tab, other_tab),
                    (width, height, maximized, zoom, split_position),
                    recent_files,
                    find_history,
                    replace_history,
                    directory_history,
                    filter_history,
                )| Session {
                    schema_version,
                    tabs,
                    active_tab,
                    window: WindowState {
                        width,
                        height,
                        maximized,
                        zoom,
                        split_position,
                    },
                    recent_files,
                    find_history,
                    replace_history,
                    directory_history,
                    filter_history,
                    other_tab,
                },
            )
    }

    fn json_value() -> impl Strategy<Value = Value> {
        let leaf = prop_oneof![
            Just(Value::Null),
            any::<bool>().prop_map(Value::from),
            any::<i64>().prop_map(Value::from),
            any::<f64>().prop_map(Value::from),
            text().prop_map(Value::from),
        ];
        leaf.prop_recursive(3, 24, 4, |inner| {
            prop_oneof![
                vec(inner.clone(), 0..4).prop_map(Value::from),
                proptest::collection::btree_map("[a-z]{1,4}", inner, 0..4)
                    .prop_map(|map| Value::Object(map.into_iter().collect())),
            ]
        })
    }

    /// Adds `key` to the session, its window, every tab and every fingerprint.
    fn add_everywhere(document: &mut Value, key: &str, value: &Value) {
        document[key] = value.clone();
        document["window"][key] = value.clone();
        for tab in document["tabs"].as_array_mut().unwrap() {
            tab[key] = value.clone();
            if tab["disk_fingerprint"].is_object() {
                tab["disk_fingerprint"][key] = value.clone();
            }
        }
    }

    /// Removes the keys `mask` picks from `object`, and sets them to their defaults in
    /// `expected` instead.
    fn remove_keys(object: &mut Value, expected: &mut Value, defaults: &Value, mask: u64) {
        let keys: Vec<String> = object.as_object().unwrap().keys().cloned().collect();
        for (bit, key) in keys.iter().enumerate() {
            if mask & (1 << (bit % 64)) != 0 {
                object.as_object_mut().unwrap().remove(key);
                expected[key] = defaults[key].clone();
            }
        }
    }

    proptest! {
        #[test]
        fn sessions_round_trip(session in session()) {
            let json = serde_json::to_string(&session).unwrap();
            prop_assert_eq!(serde_json::from_str::<Session>(&json).unwrap(), session);
        }

        #[test]
        fn unknown_fields_are_ignored(
            session in session(),
            key in "[a-z_]{1,8}",
            value in json_value(),
        ) {
            let mut document = serde_json::to_value(&session).unwrap();
            add_everywhere(&mut document, &format!("future_{key}"), &value);
            prop_assert_eq!(serde_json::from_value::<Session>(document).unwrap(), session);
        }

        #[test]
        fn missing_fields_take_their_defaults(
            session in session(),
            masks in vec(any::<u64>(), 1..8),
        ) {
            let mut document = serde_json::to_value(&session).unwrap();
            let mut expected = document.clone();
            let mut masks = masks.into_iter().cycle();
            let tab_defaults = serde_json::to_value(TabRecord::default()).unwrap();
            for index in 0..session.tabs.len() {
                remove_keys(
                    &mut document["tabs"][index],
                    &mut expected["tabs"][index],
                    &tab_defaults,
                    masks.next().unwrap(),
                );
            }
            remove_keys(
                &mut document["window"],
                &mut expected["window"],
                &serde_json::to_value(WindowState::default()).unwrap(),
                masks.next().unwrap(),
            );
            remove_keys(
                &mut document,
                &mut expected,
                &serde_json::to_value(Session::default()).unwrap(),
                masks.next().unwrap(),
            );
            let loaded = serde_json::from_value::<Session>(document).unwrap();
            prop_assert_eq!(loaded, serde_json::from_value::<Session>(expected).unwrap());
        }
    }
}
