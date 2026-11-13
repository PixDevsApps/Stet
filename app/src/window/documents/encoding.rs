//! Encodings and line endings (ADR-007, ADR-008): Reinterpret reads the file's bytes again in
//! another encoding; Convert changes what the next save writes, after checking on a worker that
//! every character exists in the target; a line-ending change is a format change only, since
//! the buffer holds LF.

use crate::editor::EditorPage;
use crate::runtime::worker;
use crate::window::dialogs::{Purpose, UnmappableChoice};
use crate::window::snapshot::PageSnapshot;
use crate::window::{PickerMode, Window};
use stet_domain::document::ReadOnly;
use stet_domain::text::Eol;
use stet_infrastructure::encoding::{
    ENCODINGS, EncodingEntry, check_encodable, entry_for_id, status_label,
};
use stet_infrastructure::fs::{DocumentFormat, EncodingChoice};

/// The encodings the Encoding menu names by item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::window) enum FixedEncoding {
    Utf8,
    Utf8Bom,
    Utf16BeBom,
    Utf16LeBom,
    /// Windows-1252.
    Ansi,
}

impl FixedEncoding {
    /// Its entry, by its [`EncodingEntry::id`].
    fn entry(self) -> Option<&'static EncodingEntry> {
        entry_for_id(match self {
            Self::Utf8 => "UTF-8",
            Self::Utf8Bom => "UTF-8+bom",
            Self::Utf16BeBom => "UTF-16BE+bom",
            Self::Utf16LeBom => "UTF-16LE+bom",
            Self::Ansi => "windows-1252",
        })
    }
}

/// The commands of the Encoding menu, Character Sets and the status bar's line-ending menu.
/// An encoding named by id comes from Character Sets or the picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::window) enum EncodingCommand {
    /// Saves the current document with these line endings.
    SetEol(Eol),
    /// Opens the status bar's encoding picker.
    Choose,
    /// Reads the file again in a fixed encoding.
    Encode(FixedEncoding),
    /// Reads the file again in the encoding with this id.
    Reinterpret(Option<String>),
    /// Makes the next save write a fixed encoding.
    ConvertTo(FixedEncoding),
    /// Makes the next save write the encoding with this id.
    Convert(Option<String>),
}

impl Window {
    /// Runs an Encoding-menu or line-ending command on the current document.
    pub(in crate::window) fn run_encoding(&self, command: EncodingCommand) {
        let Some(page) = self.current_page() else {
            return;
        };
        match command {
            EncodingCommand::SetEol(eol) => self.convert_eol(&page, eol),
            EncodingCommand::Choose => self.choose_encoding(None),
            EncodingCommand::Encode(fixed) => self.spawn_reinterpret(page, fixed.entry()),
            EncodingCommand::Reinterpret(id) => {
                self.spawn_reinterpret(page, id.as_deref().and_then(entry_for_id));
            }
            EncodingCommand::ConvertTo(fixed) => self.spawn_convert(page, fixed.entry()),
            EncodingCommand::Convert(id) => {
                self.spawn_convert(page, id.as_deref().and_then(entry_for_id));
            }
        }
    }

    fn spawn_reinterpret(&self, page: EditorPage, entry: Option<&'static EncodingEntry>) {
        if let Some(entry) = entry {
            let window = self.clone();
            self.spawn(async move { window.reinterpret(&page, entry).await });
        }
    }

    fn spawn_convert(&self, page: EditorPage, entry: Option<&'static EncodingEntry>) {
        if let Some(entry) = entry {
            let window = self.clone();
            self.spawn(async move { window.convert(&page, entry).await });
        }
    }

    /// Opens the status bar's encoding picker, in `mode` when given.
    pub(in crate::window) fn choose_encoding(&self, mode: Option<PickerMode>) {
        self.update_status();
        if let Some(mode) = mode {
            self.inner.encodings.set_mode(mode);
        }
        self.inner.status.encoding.popup();
    }

    /// Reads the file again as `entry`, after asking when that discards unsaved changes.
    async fn reinterpret(&self, page: &EditorPage, entry: &'static EncodingEntry) {
        if page.path().is_none() || page.is_loading() {
            return;
        }
        let name = page.name();
        if page.is_dirty() {
            let confirmed = self
                .confirm(
                    &format!("Reinterpret “{name}” as {}?", entry.short_label()),
                    "This reads the file from disk again and discards your unsaved changes.",
                    "_Reinterpret",
                    true,
                )
                .await;
            if !confirmed {
                return;
            }
        }
        let choice = EncodingChoice::from(entry);
        if self.reload_full(page, Some(choice)).await {
            self.toast(&format!("Reading “{name}” as {}", entry.short_label()));
        }
    }

    /// Makes the next save write `entry`, when every character exists in it; otherwise the
    /// unmappable dialog lists the ones that don't.
    async fn convert(&self, page: &EditorPage, entry: &'static EncodingEntry) {
        if page.is_loading() || page.read_only() == Some(ReadOnly::DisplayBreaks) {
            return;
        }
        let format = page.state().save.format;
        let label = entry.short_label();
        if format.encoding == entry.encoding && format.bom == entry.bom {
            self.toast(&format!("“{}” is already {label}", page.name()));
            return;
        }
        let (snapshot, text) = PageSnapshot::take(page);
        let encoding = entry.encoding;
        let map_placeholder = format.map_placeholder;
        let checked = worker::run(move || check_encodable(&text, encoding, map_placeholder)).await;
        if snapshot.edited() {
            self.toast("The text changed while it was checked; convert again");
            return;
        }
        match checked {
            Some(Ok(())) => self.set_encoding(page, entry),
            Some(Err(error)) => {
                match self
                    .ask_unmappable(page, &error, entry.bom, Purpose::Convert)
                    .await
                {
                    UnmappableChoice::UseUtf8 => {
                        if let Some(utf8) = entry_for_id("UTF-8") {
                            self.set_encoding(page, utf8);
                        }
                    }
                    UnmappableChoice::GoTo(offset) => page.select_char(offset),
                    UnmappableChoice::Cancel => {}
                }
            }
            None => self.toast(&format!("Could not check “{}”", page.name())),
        }
    }

    fn set_encoding(&self, page: &EditorPage, entry: &EncodingEntry) {
        page.convert_encoding(entry.encoding, entry.bom);
        self.refresh_banners(page);
        self.refresh_page(page);
        self.toast(&format!(
            "“{}” is saved as {} from now on",
            page.name(),
            entry.short_label()
        ));
    }

    /// Makes the next save write `eol` everywhere; this also settles mixed line endings.
    pub(in crate::window) fn convert_eol(&self, page: &EditorPage, eol: Eol) {
        if page.is_loading() || page.read_only() == Some(ReadOnly::DisplayBreaks) {
            return;
        }
        page.convert_eol(eol);
        self.refresh_banners(page);
        self.refresh_page(page);
    }
}

/// The status bar's text for a document's format: its encoding and line ending.
pub fn format_labels(format: &DocumentFormat, mixed_eol: bool) -> (String, &'static str) {
    let eol = if mixed_eol {
        "Mixed"
    } else {
        format.eol.label()
    };
    (status_label(format.encoding, format.bom), eol)
}

/// Every encoding entry, for the menus: (group label, entries) in display order.
pub fn grouped_entries() -> Vec<(&'static str, Vec<&'static EncodingEntry>)> {
    let mut groups: Vec<(&'static str, Vec<&'static EncodingEntry>)> = Vec::new();
    for entry in &ENCODINGS {
        match groups.last_mut() {
            Some((label, entries)) if *label == entry.group.label() => entries.push(entry),
            _ => groups.push((entry.group.label(), vec![entry])),
        }
    }
    groups
}
