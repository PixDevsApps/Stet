//! A tab's banners (DESIGN.md "Window layout"): what the document's state says needs
//! attention, most urgent on top, with the buttons that act on it. Loading progress is shown
//! by the editor itself. Every tab of a document (a clone in the other view, M7) shows them.

use super::Window;
use crate::banner::{BannerButton, BannerKind};
use crate::editor::{DiskState, DocState, EditorPage, ReadOnly, TextOrigin, Unwritable};
use gtk4::prelude::*;
use std::rc::Rc;
use stet_domain::document::sudoedit_command;
use stet_domain::text::{DISPLAY_BREAK_CHARS, Eol};
use stet_domain::view::{HIGHLIGHT_MAX_BYTES, HIGHLIGHT_MAX_LINES};
use stet_infrastructure::encoding::status_label;
use stet_infrastructure::fs::MIB;

use super::dialogs::grouped;

impl Window {
    /// Shows exactly the banners `page`'s state calls for, on every tab of its document.
    pub(super) fn refresh_banners(&self, page: &EditorPage) {
        for page in page.document_pages() {
            self.refresh_page_banners(&page);
        }
    }

    fn refresh_page_banners(&self, page: &EditorPage) {
        let state = page.state();
        let name = page.name();
        self.disk_banner(page, &state, &name);
        self.read_only_banner(page, &state, &name);
        self.binary_banner(page, &state, &name);
        self.encoding_banners(page, &state, &name);
        self.mixed_eol_banner(page, &state);
        match state.origin {
            TextOrigin::File => page.hide_banner(BannerKind::LongLines),
            TextOrigin::Formatted(kind) => page.show_banner(
                BannerKind::LongLines,
                &format!(
                    "“{name}” has very long lines and was opened formatted as {}: the text \
                     differs from the file's bytes, and saving asks before writing it.",
                    kind.label()
                ),
                Vec::new(),
            ),
            TextOrigin::DisplayBreaks => page.show_banner(
                BannerKind::LongLines,
                &format!(
                    "“{name}” has very long lines, broken every {} characters for display. The \
                     text isn't the file's, so it can't be saved.",
                    grouped(DISPLAY_BREAK_CHARS)
                ),
                Vec::new(),
            ),
        }
        if state.large() {
            let size = state.baseline.map_or(0, |baseline| baseline.size);
            page.show_banner(
                BannerKind::LargeFile,
                &format!(
                    "Large file ({:.1} MiB): syntax highlighting, word wrap, whitespace and \
                     bracket matching are off.",
                    size as f64 / MIB as f64
                ),
                Vec::new(),
            );
        } else {
            page.hide_banner(BannerKind::LargeFile);
        }
        if state.highlight_limited && !page.highlighting() && page.language().is_some() {
            let button = self.banner_button(page, "Highlight Anyway", |window, page| {
                page.update_state(|state| state.highlight_limited = false);
                page.set_highlighting(true);
                window.refresh_banners(&page);
            });
            page.show_banner(
                BannerKind::Highlighting,
                &format!(
                    "Syntax highlighting is off for files over {} MiB or {} lines, so that \
                     typing stays fast.",
                    HIGHLIGHT_MAX_BYTES as u64 / MIB,
                    grouped(HIGHLIGHT_MAX_LINES)
                ),
                vec![button],
            );
        } else {
            page.hide_banner(BannerKind::Highlighting);
        }
    }

    fn disk_banner(&self, page: &EditorPage, state: &DocState, name: &str) {
        match state.disk {
            DiskState::Same => {
                page.update_session(|session| session.conflict = false);
                page.hide_banner(BannerKind::Disk);
            }
            DiskState::Changed => {
                // A restored backup whose file changed while Stet was closed (M2) says so.
                // Compare shows the unsaved text beside the file on disk (M7).
                let restored = page.session().conflict;
                let reload_label = if restored {
                    "Load Disk Version"
                } else {
                    "Reload"
                };
                let reload = self.banner_button(page, reload_label, |window, page| {
                    let target = window.clone();
                    window.spawn(async move {
                        target.reload(&page, false).await;
                    });
                });
                let keep = self.banner_button(page, "Keep Mine", |window, page| {
                    window.keep_mine(&page);
                });
                let compare = self.banner_button(page, "Compare", |window, page| {
                    let target = window.clone();
                    window.spawn(async move { target.compare_with_saved(page).await });
                });
                let (title, buttons) = if restored {
                    (
                        format!("The file “{name}” changed on disk since your unsaved changes."),
                        vec![compare, keep, reload],
                    )
                } else {
                    (
                        format!("“{name}” changed on disk."),
                        vec![compare, reload, keep],
                    )
                };
                page.show_banner(BannerKind::Disk, &title, buttons);
            }
            DiskState::Deleted { .. } => {
                let save = self.banner_button(page, "Save", |window, page| {
                    let target = window.clone();
                    window.spawn(async move {
                        target.save(&page).await;
                    });
                });
                page.show_banner(
                    BannerKind::Disk,
                    &format!("“{name}” is no longer on disk. Save it to keep it."),
                    vec![save],
                );
            }
        }
    }

    fn read_only_banner(&self, page: &EditorPage, state: &DocState, name: &str) {
        let path = state
            .path
            .as_deref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        let command = sudoedit_command(&path);
        let copy = |window: &Window| {
            let command = command.clone();
            window.banner_button(page, "Copy Command", move |window, page| {
                page.clipboard().set_text(&command);
                window.toast("Copied the sudoedit command");
            })
        };
        if state.read_only == Some(ReadOnly::NotWritable) {
            let edit = self.banner_button(page, "Edit Anyway", |window, page| {
                page.set_read_only(None);
                window.refresh_banners(&page);
            });
            page.show_banner(
                BannerKind::ReadOnly,
                &format!(
                    "You may not write “{name}”, so it is open read-only. To edit it as root, \
                     run: {command}"
                ),
                vec![copy(self), edit],
            );
            return;
        }
        match state.unwritable {
            Some(Unwritable::Permission) => page.show_banner(
                BannerKind::ReadOnly,
                &format!(
                    "You may not write “{name}”. Save a copy with Save As, or edit it as root: \
                     {command}"
                ),
                vec![copy(self)],
            ),
            Some(Unwritable::ReadOnlyFilesystem) => page.show_banner(
                BannerKind::ReadOnly,
                &format!("“{name}” is on a read-only file system. Save a copy with Save As."),
                Vec::new(),
            ),
            None => page.hide_banner(BannerKind::ReadOnly),
        }
    }

    fn binary_banner(&self, page: &EditorPage, state: &DocState, name: &str) {
        if state.read_only != Some(ReadOnly::Binary) {
            page.hide_banner(BannerKind::Binary);
            return;
        }
        if state.lossy() || state.mixed_eol {
            page.show_banner(
                BannerKind::Binary,
                &format!(
                    "“{name}” looks binary and can't be saved exactly as it was, so it is \
                     open read-only."
                ),
                Vec::new(),
            );
            return;
        }
        let edit = self.banner_button(page, "Edit Anyway", |window, page| {
            page.set_read_only(None);
            window.refresh_banners(&page);
        });
        page.show_banner(
            BannerKind::Binary,
            &format!(
                "“{name}” looks binary (it has NUL bytes), so it is open read-only. NUL bytes \
                 show as ␀ and are saved as NUL."
            ),
            vec![edit],
        );
    }

    fn encoding_banners(&self, page: &EditorPage, state: &DocState, name: &str) {
        let both_nuls = state.placeholder_conflict && state.nul_count > 0;
        if both_nuls {
            page.show_banner(
                BannerKind::Placeholder,
                &format!(
                    "“{name}” has both NUL bytes and ␀ characters. Both show as ␀ and are \
                     saved as ␀, so the NUL bytes can't be saved as they were."
                ),
                Vec::new(),
            );
        } else {
            page.hide_banner(BannerKind::Placeholder);
        }
        if !state.lossy() || both_nuls {
            page.hide_banner(BannerKind::Lossy);
            return;
        }
        let encoding = status_label(state.format.encoding, state.format.bom);
        let reason = if state.had_errors {
            format!("some bytes aren't valid {encoding} and show as �")
        } else {
            format!("{encoding} doesn't write some of its characters back as the same bytes")
        };
        let reinterpret = self.banner_button(page, "Reinterpret…", |window, _| {
            window.choose_encoding(Some(super::PickerMode::Reinterpret));
        });
        page.show_banner(
            BannerKind::Lossy,
            &format!("This file can't be saved exactly as it was: {reason}."),
            vec![reinterpret],
        );
    }

    fn mixed_eol_banner(&self, page: &EditorPage, state: &DocState) {
        if !state.mixed_eol || state.mixed_kept {
            page.hide_banner(BannerKind::MixedEol);
            return;
        }
        let stats = state.eol_stats;
        let counts: Vec<String> = [
            (stats.lf, Eol::Lf),
            (stats.crlf, Eol::CrLf),
            (stats.cr, Eol::Cr),
        ]
        .into_iter()
        .filter(|(count, _)| *count > 0)
        .map(|(count, eol)| format!("{} {}", grouped(count), eol.label()))
        .collect();
        let dominant = state.format.eol;
        let normalize = self.banner_button(
            page,
            &format!("Normalize to {}", dominant.label()),
            move |window, page| window.convert_eol(&page, dominant),
        );
        let keep = self.banner_button(page, "Keep", |window, page| {
            page.update_state(|state| state.mixed_kept = true);
            window.refresh_banners(&page);
        });
        page.show_banner(
            BannerKind::MixedEol,
            &format!(
                "Mixed line endings ({}): saving writes {} everywhere.",
                counts.join(", "),
                dominant.label()
            ),
            vec![normalize, keep],
        );
    }

    /// A banner button that runs `run` with this window and `page`, while both exist.
    fn banner_button(
        &self,
        page: &EditorPage,
        label: &str,
        run: impl Fn(Window, EditorPage) + 'static,
    ) -> BannerButton {
        let inner = Rc::downgrade(&self.inner);
        let page = page.downgrade();
        BannerButton::new(label, move || {
            if let (Some(inner), Some(page)) = (inner.upgrade(), page.upgrade()) {
                let window = Window { inner };
                run(window.clone(), page);
                window.restore_focus_later();
            }
        })
    }
}
