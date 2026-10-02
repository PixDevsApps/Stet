//! Encodings and line endings (ADR-007, ADR-008): Reinterpret reads the file's bytes again in
//! another encoding; Convert changes what the next save writes, after checking on a worker that
//! every character exists in the target; a line-ending change is a format change only, since
//! the buffer holds LF.

use super::dialogs::{Purpose, UnmappableChoice};
use super::{PickerMode, Window};
use crate::editor::{EditorPage, ReadOnly};
use crate::worker;
use stet_domain::actions::ActionId;
use stet_domain::text::Eol;
use stet_infrastructure::encoding::{
    ENCODINGS, EncodingEntry, check_encodable, entry_for_id, status_label,
};
use stet_infrastructure::fs::{DocumentFormat, EncodingChoice};

/// The fixed entries of the Encoding menu, by their [`EncodingEntry::id`].
fn fixed_entry(id: ActionId) -> Option<&'static EncodingEntry> {
    let entry = match id {
        ActionId::EncodeUtf8 | ActionId::ConvertToUtf8 => "UTF-8",
        ActionId::EncodeUtf8Bom | ActionId::ConvertToUtf8Bom => "UTF-8+bom",
        ActionId::EncodeUtf16BeBom | ActionId::ConvertToUtf16BeBom => "UTF-16BE+bom",
        ActionId::EncodeUtf16LeBom | ActionId::ConvertToUtf16LeBom => "UTF-16LE+bom",
        ActionId::ConvertToAnsi => "windows-1252",
        _ => return None,
    };
    entry_for_id(entry)
}

impl Window {
    /// Runs an Encoding-menu or line-ending action; `param` is an encoding id.
    pub(super) fn run_encoding_action(&self, id: ActionId, param: Option<String>) {
        let Some(page) = self.current_page() else {
            return;
        };
        let entry = match id {
            ActionId::Reinterpret | ActionId::ConvertEncoding => {
                param.as_deref().and_then(entry_for_id)
            }
            _ => fixed_entry(id),
        };
        match id {
            ActionId::EolCrLf => self.convert_eol(&page, Eol::CrLf),
            ActionId::EolLf => self.convert_eol(&page, Eol::Lf),
            ActionId::EolCr => self.convert_eol(&page, Eol::Cr),
            ActionId::ChooseEncoding => self.choose_encoding(None),
            ActionId::EncodeUtf8
            | ActionId::EncodeUtf8Bom
            | ActionId::EncodeUtf16BeBom
            | ActionId::EncodeUtf16LeBom
            | ActionId::Reinterpret => {
                if let Some(entry) = entry {
                    let window = self.clone();
                    self.spawn(async move { window.reinterpret(&page, entry).await });
                }
            }
            _ => {
                if let Some(entry) = entry {
                    let window = self.clone();
                    self.spawn(async move { window.convert(&page, entry).await });
                }
            }
        }
    }

    /// Opens the status bar's encoding picker, in `mode` when given.
    pub(super) fn choose_encoding(&self, mode: Option<PickerMode>) {
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
        let format = page.state().format;
        let label = entry.short_label();
        if format.encoding == entry.encoding && format.bom == entry.bom {
            self.toast(&format!("“{}” is already {label}", page.name()));
            return;
        }
        let text = page.text();
        let revision = page.revision();
        let encoding = entry.encoding;
        let map_placeholder = format.map_placeholder;
        let checked = worker::run(move || check_encodable(&text, encoding, map_placeholder)).await;
        if page.revision() != revision {
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
        let changed =
            page.state().format.encoding != entry.encoding || page.state().format.bom != entry.bom;
        page.update_state(|state| {
            state.format.encoding = entry.encoding;
            state.format.bom = entry.bom;
            state.format_changed |= changed;
        });
        self.refresh_banners(page);
        self.refresh_page(page);
        self.toast(&format!(
            "“{}” is saved as {} from now on",
            page.name(),
            entry.short_label()
        ));
    }

    /// Makes the next save write `eol` everywhere; this also settles mixed line endings.
    pub(super) fn convert_eol(&self, page: &EditorPage, eol: Eol) {
        if page.is_loading() || page.read_only() == Some(ReadOnly::DisplayBreaks) {
            return;
        }
        let state = page.state();
        let changed = state.format.eol != eol || state.mixed_eol;
        page.update_state(|state| {
            state.format.eol = eol;
            state.mixed_eol = false;
            state.mixed_kept = false;
            state.format_changed |= changed;
        });
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
