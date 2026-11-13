//! Pinned tabs: AdwTabView's pinned pages, which stay at the start of the strip as
//! icon-only tabs. Close All skips them; closing a pinned tab itself still works and asks when
//! it has unsaved changes. The session keeps them (`TabRecord::pinned`).
//!
//! The tab strip's context menu (`TAB_MENU`) starts with Pin Tab or Unpin Tab for the
//! clicked tab; only the one that applies shows. Close All, Close Others and Close to the Right
//! skip pinned tabs ([`Window::closable_by_bulk_commands`]).

use super::Window;
use crate::editor::EditorPage;
use gtk4::gio;
use libadwaita::prelude::*;
use stet_domain::actions::ActionId;

/// A pinned tab with unsaved changes shows this instead of its file type's icon: pinned tabs
/// have no title for the dirty dot.
const DIRTY_ICON: &str = "media-record-symbolic";

/// The tab menu's Pin Tab and Unpin Tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PinCommand {
    Pin,
    Unpin,
}

impl Window {
    /// The tab a pin command acts on: the one whose menu is open, else the current one.
    fn pin_target(&self) -> Option<EditorPage> {
        self.context_page()
    }

    pub fn is_pinned(&self, page: &EditorPage) -> bool {
        self.tab_page(page)
            .is_some_and(|tab_page| tab_page.is_pinned())
    }

    /// Pins or unpins `page`: a pinned tab moves to the end of the pinned ones at the start.
    pub fn set_pinned(&self, page: &EditorPage, pinned: bool) {
        let Some((view, tab_page)) = self.locate(page) else {
            return;
        };
        if tab_page.is_pinned() != pinned {
            self.inner.views[view]
                .tabs
                .set_page_pinned(&tab_page, pinned);
        }
        self.update_pin_icon(page);
        self.update_pin_actions();
    }

    /// Pin Tab and Unpin Tab.
    pub(super) fn run_pin(&self, command: PinCommand) {
        if let Some(page) = self.pin_target() {
            self.set_pinned(&page, command == PinCommand::Pin);
        }
    }

    /// Only the command that applies to the tab is enabled; the tab menu hides the other.
    pub(super) fn update_pin_actions(&self) {
        let pinned = self.pin_target().map(|page| self.is_pinned(&page));
        for (id, enabled) in [
            (ActionId::PinTab, pinned == Some(false)),
            (ActionId::UnpinTab, pinned == Some(true)),
        ] {
            if let Some(action) = self.action(id) {
                action.set_enabled(enabled);
            }
        }
    }

    /// A pinned tab shows only an icon: its file type's, or a dot while it has unsaved
    /// changes.
    pub(super) fn update_pin_icon(&self, page: &EditorPage) {
        let Some(tab_page) = self.tab_page(page) else {
            return;
        };
        if !tab_page.is_pinned() {
            tab_page.set_icon(None::<&gio::Icon>);
            return;
        }
        let icon = if Self::has_unsaved_changes(page) {
            gio::ThemedIcon::new(DIRTY_ICON).upcast::<gio::Icon>()
        } else {
            let name = page
                .path()
                .and_then(|path| {
                    path.file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                })
                .unwrap_or_default();
            let (content_type, _) = gio::content_type_guess(Some(name.as_str()), None);
            gio::content_type_get_symbolic_icon(&content_type)
        };
        tab_page.set_icon(Some(&icon));
    }

    /// The tabs that closing commands may close: every tab but the pinned ones. Close All
    /// uses it; Close Others and Close to the Right must too.
    pub fn closable_by_bulk_commands(&self, pages: Vec<EditorPage>) -> Vec<EditorPage> {
        pages
            .into_iter()
            .filter(|page| !self.is_pinned(page))
            .collect()
    }
}
