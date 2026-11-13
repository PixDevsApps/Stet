//! A tab's banners (DESIGN.md "Window layout"): the domain decides from the document's state
//! which ones it needs and what they say ([`banners_for`]); this module shows them on every tab
//! of the document and wires their buttons. Loading progress is shown by the editor itself.

use crate::editor::BannerButton;
use crate::editor::EditorPage;
use crate::window::Window;
use gtk4::prelude::*;
use std::rc::Rc;
use stet_domain::document::DiskState;
use stet_domain::document::banners::{
    BannerAction, BannerFacts, BannerKind, HighlightLimits, banners_for,
};
use stet_infrastructure::encoding::status_label;

impl Window {
    /// Shows exactly the banners `page`'s state calls for, on every tab of its document.
    pub(in crate::window) fn refresh_banners(&self, page: &EditorPage) {
        for page in page.document_pages() {
            self.refresh_page_banners(&page);
        }
    }

    fn refresh_page_banners(&self, page: &EditorPage) {
        let facts = self.banner_facts(page);
        if facts.disk == DiskState::Same {
            page.update_session(|session| session.conflict = false);
        }
        let specs = banners_for(&facts);
        for kind in BannerKind::FROM_STATE {
            match specs.iter().find(|spec| spec.kind == kind) {
                Some(spec) => {
                    let buttons = spec
                        .actions
                        .iter()
                        .map(|action| self.banner_button(page, action))
                        .collect();
                    page.show_banner(kind, &spec.title, buttons);
                }
                None => page.hide_banner(kind),
            }
        }
    }

    /// What the banners need to know about `page`'s document.
    fn banner_facts(&self, page: &EditorPage) -> BannerFacts {
        let state = page.state();
        let held = state.highlight_limited && !page.highlighting() && page.language().is_some();
        BannerFacts {
            name: page.name(),
            restored_conflict: page.session().conflict,
            disk: state.disk,
            read_only: state.read_only(),
            unwritable: state.unwritable,
            loss: state.loss,
            encoding: status_label(state.save.format.encoding, state.save.format.bom),
            nul_count: state.nul_count(),
            placeholder_conflict: state.placeholder_conflict(),
            mixed_eol: state.eol.mixed,
            mixed_kept: state.eol.kept,
            eol_stats: state.eol_stats(),
            eol: state.save.format.eol,
            origin: state.origin,
            large_file: state
                .large()
                .then(|| state.baseline.map_or(0, |baseline| baseline.size)),
            highlight_limited: held.then(|| {
                let settings = self.settings();
                HighlightLimits {
                    max_bytes: settings.highlight_max_bytes,
                    max_lines: settings.highlight_max_lines,
                }
            }),
            path: state.path,
        }
    }

    /// The button for `action` on `page`'s banner.
    fn banner_button(&self, page: &EditorPage, action: &BannerAction) -> BannerButton {
        let label = action.label();
        match action.clone() {
            BannerAction::Reload | BannerAction::LoadDiskVersion => {
                self.on_banner(page, label, |window, page| {
                    let target = window.clone();
                    window.spawn(async move {
                        target.reload(&page, false).await;
                    });
                })
            }
            BannerAction::KeepMine => {
                self.on_banner(page, label, |window, page| window.keep_mine(&page))
            }
            BannerAction::Compare => self.on_banner(page, label, |window, page| {
                let target = window.clone();
                window.spawn(async move { target.compare_with_saved(page).await });
            }),
            BannerAction::Save => self.on_banner(page, label, |window, page| {
                let target = window.clone();
                window.spawn(async move {
                    target.save(&page).await;
                });
            }),
            BannerAction::CopyCommand(command) => {
                self.on_banner(page, label, move |window, page| {
                    page.clipboard().set_text(&command);
                    window.toast("Copied the sudoedit command");
                })
            }
            BannerAction::EditAnyway => self.on_banner(page, label, |window, page| {
                page.edit_anyway();
                window.refresh_banners(&page);
            }),
            BannerAction::Reinterpret => self.on_banner(page, label, |window, _| {
                window.choose_encoding(Some(crate::window::PickerMode::Reinterpret));
            }),
            BannerAction::Normalize(eol) => self.on_banner(page, label, move |window, page| {
                window.convert_eol(&page, eol);
            }),
            BannerAction::KeepMixedEndings => self.on_banner(page, label, |window, page| {
                page.keep_mixed_endings();
                window.refresh_banners(&page);
            }),
            BannerAction::HighlightAnyway => self.on_banner(page, label, |window, page| {
                page.highlight_anyway();
                window.refresh_banners(&page);
            }),
        }
    }

    /// A banner button that runs `run` with this window and `page`, while both exist.
    fn on_banner(
        &self,
        page: &EditorPage,
        label: String,
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
