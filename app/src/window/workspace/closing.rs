//! Closing tabs and bringing them back: a tab with unsaved changes asks first, unless its
//! document stays open in the other view; Close All asks once for every tab it closes; Restore
//! Closed Tab reopens the last closed tabs, an untitled one with its text.

use crate::convert::to_u32;
use crate::editor::EditorPage;
use crate::window::dialogs::SaveChoice;
use crate::window::{Location, Window, page_of};
use libadwaita as adw;
use sourceview5::prelude::*;
use stet_domain::document::location::LineCol;
use stet_domain::nav::ClosedTab;

impl Window {
    /// A view asks to close `tab_page`: at once, or after the user answered.
    pub(in crate::window) fn on_close_page(&self, tabs: &adw::TabView, tab_page: &adw::TabPage) {
        let page = page_of(tab_page);
        let tabs = tabs.clone();
        // A clone closes alone: its document stays open in the other view.
        if self.other_page(&page).is_some() {
            self.finish_close(&tabs, tab_page, true);
            return;
        }
        let unsaved = Self::has_unsaved_changes(&page);
        if !unsaved || self.inner.tabs.is_closing_without_prompt() {
            self.remember_closed(&page);
            self.tab_closed(&page, unsaved || page.is_loading());
            self.finish_close(&tabs, tab_page, true);
            return;
        }
        let window = self.clone();
        let tab_page = tab_page.clone();
        self.spawn(async move {
            window.select(&page);
            let choice = window.ask_save_changes(&[page.name()]).await;
            let confirmed = match choice {
                SaveChoice::Save => window.save(&page).await,
                SaveChoice::Discard => true,
                SaveChoice::Cancel => false,
            };
            if confirmed {
                window.remember_closed(&page);
                window.tab_closed(&page, choice == SaveChoice::Discard);
            }
            window.finish_close(&tabs, &tab_page, confirmed);
        });
    }

    /// Ends a tab's close, which detaches it from its view when `confirmed`.
    fn finish_close(&self, tabs: &adw::TabView, tab_page: &adw::TabPage, confirmed: bool) {
        let _finishing = confirmed.then(|| self.inner.tabs.finishing_close());
        tabs.close_page_finish(tab_page, confirmed);
    }

    /// A tab left its view: closed, or moved to the other view (Move to Other View, or dragged
    /// to the other view's strip), which attaches it there next.
    pub(in crate::window) fn on_page_detached(&self, page: &EditorPage) {
        let tabs = &self.inner.tabs;
        if tabs.is_moving() || !tabs.is_closing() {
            return;
        }
        page.load_changed().notify();
        self.on_nav_page_detached(page);
        self.compare_page_closed(page);
        if self.pages().is_empty() {
            if !self.inner.tabs.is_closing_window() {
                self.new_untitled();
            }
        } else {
            self.update_layout();
        }
        self.update_actions();
    }

    pub(in crate::window) fn close_without_prompt(&self, page: &EditorPage) {
        if let Some((index, tab_page)) = self.locate(page) {
            let _closing = self.inner.tabs.closing_without_prompt();
            self.inner.views[index].tabs.close_page(&tab_page);
        }
    }

    /// Closes every tab of `page`'s document without asking.
    pub(in crate::window) fn close_document(&self, page: &EditorPage) {
        for view_page in page.document_pages() {
            self.close_without_prompt(&view_page);
        }
    }

    /// Another tab that shows `page`'s document, in either view.
    pub(in crate::window) fn other_page(&self, page: &EditorPage) -> Option<EditorPage> {
        page.document_pages()
            .into_iter()
            .find(|other| other != page && self.tab_page(other).is_some())
    }

    /// The documents that closing `pages` closes, one tab each: those with no tab left open.
    pub(in crate::window) fn closing_documents(&self, pages: &[EditorPage]) -> Vec<EditorPage> {
        let mut documents: Vec<EditorPage> = Vec::new();
        for page in pages {
            if documents.iter().any(|known| known.same_document(page)) {
                continue;
            }
            let all_closing = page
                .document_pages()
                .iter()
                .filter(|other| self.tab_page(other).is_some())
                .all(|other| pages.contains(other));
            if all_closing {
                documents.push(page.clone());
            }
        }
        documents
    }

    /// Remembers `page` for Restore Closed Tab: a file, or an untitled tab with text.
    fn remember_closed(&self, page: &EditorPage) {
        if page.is_loading() || page.state().temporary.is_some() {
            return;
        }
        let position = page.cursor();
        let entry = match page.path() {
            Some(path) => ClosedTab::File { path, position },
            None => {
                let text = page.text();
                if text.is_empty() {
                    return;
                }
                ClosedTab::Untitled {
                    text,
                    position,
                    language: page.language().map(|language| language.id().to_string()),
                    name: page.state().custom_name,
                }
            }
        };
        self.inner.tabs.remember_closed(entry);
    }

    /// Close All: every tab but the pinned ones, asking once about those with unsaved changes.
    pub(in crate::window) async fn close_all(&self) {
        let closing = self.closable_by_bulk_commands(self.pages());
        let dirty: Vec<EditorPage> = self
            .closing_documents(&closing)
            .into_iter()
            .filter(Self::has_unsaved_changes)
            .collect();
        if !dirty.is_empty() {
            let names: Vec<String> = dirty.iter().map(EditorPage::name).collect();
            match self.ask_save_changes(&names).await {
                SaveChoice::Save => {
                    for page in &dirty {
                        self.select(page);
                        if !self.save(page).await {
                            return;
                        }
                    }
                }
                SaveChoice::Discard => {}
                SaveChoice::Cancel => return,
            }
        }
        for page in closing {
            self.close_without_prompt(&page);
        }
    }

    pub fn restore_closed(&self) {
        let Some(entry) = self.inner.tabs.take_closed() else {
            return;
        };
        match entry {
            ClosedTab::File { path, position } => {
                self.open_locations(vec![Location {
                    path,
                    position: Some(LineCol::new(
                        to_u32(position.line + 1),
                        Some(to_u32(position.column + 1)),
                    )),
                    create: false,
                }]);
            }
            ClosedTab::Untitled {
                text,
                position,
                language,
                name,
            } => {
                let page = self.new_untitled();
                page.set_custom_name(name);
                let buffer = page.buffer();
                buffer.begin_irreversible_action();
                buffer.set_text(&text);
                buffer.end_irreversible_action();
                buffer.set_modified(true);
                self.set_language(&page, language.as_deref());
                page.go_to(position);
                self.refresh_page(&page);
            }
        }
        self.update_actions();
    }
}
