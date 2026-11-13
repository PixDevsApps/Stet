//! The window's tabs as editor pages: finding them in the views, adding a tab with the window's
//! hooks on its editor and buffer, selecting one, and what follows when another becomes
//! current. A document shown in both views has a tab, and a page, in each.

use crate::actions;
use crate::editor::{DocState, EditorPage};
use crate::window::{Window, page_of, view_pages};
use gtk4 as gtk;
use gtk4::{gdk, glib};
use libadwaita as adw;
use libadwaita::prelude::*;
use sourceview5::prelude::*;
use stet_domain::actions::ActionId;
use stet_domain::document::{next_untitled_number, tab_title, window_title};

/// Every editor follows the theme's colour scheme.
pub(in crate::window) fn connect(window: &Window) {
    window
        .inner
        .shared
        .appearance
        .connect_scheme_changed(glib::clone!(
            #[weak]
            window,
            move |scheme| {
                for page in window.pages() {
                    page.set_scheme(scheme);
                }
            }
        ));
}

impl Window {
    /// Every tab: the main view's, then the second view's.
    pub fn pages(&self) -> Vec<EditorPage> {
        self.inner
            .views
            .iter()
            .flat_map(|view| view_pages(&view.tabs))
            .collect()
    }

    /// One tab per open document, in tab order: the current tab for its document, else the
    /// first that shows it. Operations on documents (saving, searching every document) use
    /// it, so a document in both views counts once.
    pub fn documents(&self) -> Vec<EditorPage> {
        let current = self.current_page();
        let mut documents: Vec<EditorPage> = Vec::new();
        for page in self.pages() {
            if documents.iter().any(|known| known.same_document(&page)) {
                continue;
            }
            match &current {
                Some(current) if current.same_document(&page) => documents.push(current.clone()),
                _ => documents.push(page),
            }
        }
        documents
    }

    /// The tab in front of the active view: where commands act.
    pub fn current_page(&self) -> Option<EditorPage> {
        self.inner.views[self.inner.active.get()]
            .tabs
            .selected_page()
            .map(|page| page_of(&page))
    }

    pub(in crate::window) fn tab_page(&self, page: &EditorPage) -> Option<adw::TabPage> {
        self.locate(page).map(|(_, tab_page)| tab_page)
    }

    /// The view that shows `page`, and its tab there.
    pub(in crate::window) fn locate(&self, page: &EditorPage) -> Option<(usize, adw::TabPage)> {
        let widget = page.upcast_ref::<gtk::Widget>();
        self.inner
            .views
            .iter()
            .enumerate()
            .find_map(|(index, view)| {
                let tabs = &view.tabs;
                (0..tabs.n_pages())
                    .map(|position| tabs.nth_page(position))
                    .find(|tab_page| tab_page.child() == *widget)
                    .map(|tab_page| (index, tab_page))
            })
    }

    /// Shows `page` and makes its view the active one.
    pub fn select(&self, page: &EditorPage) {
        let Some((index, tab_page)) = self.locate(page) else {
            return;
        };
        let tabs = &self.inner.views[index].tabs;
        let changes = tabs.selected_page().as_ref() != Some(&tab_page);
        if index != self.inner.active.get() {
            self.inner.active.set(index);
            self.style_views();
            if !changes {
                self.current_changed(true);
            }
        }
        if changes {
            tabs.set_selected_page(&tab_page);
        }
    }

    pub(in crate::window) fn add_page(&self, state: DocState) -> EditorPage {
        self.add_page_with(state, true)
    }

    /// Appends a tab for `state` to the active view, selecting it when `select` is set (a
    /// restored session's tabs are not, so that only the active one loads).
    pub(in crate::window) fn add_page_with(&self, state: DocState, select: bool) -> EditorPage {
        self.add_page_in(self.target_view(), state, select)
    }

    /// The view new tabs open in: the active one.
    fn target_view(&self) -> usize {
        let active = self.inner.active.get();
        if self.inner.views[active].tabs.n_pages() > 0 || self.pages().is_empty() {
            active
        } else {
            1 - active
        }
    }

    /// Appends a tab for `state` to `view`.
    pub(in crate::window) fn add_page_in(
        &self,
        view: usize,
        state: DocState,
        select: bool,
    ) -> EditorPage {
        let page = EditorPage::new(
            state,
            self.inner.shared.appearance.scheme().as_ref(),
            self.inner.settings.get(),
        );
        self.attach_page(view, &page, select);
        page
    }

    /// Appends `page`, a new tab or a clone, to `view`, with the window's hooks.
    pub(in crate::window) fn attach_page(&self, view: usize, page: &EditorPage, select: bool) {
        self.install_page(page);
        self.on_nav_page_attached(page);
        let tabs = &self.inner.views[view].tabs;
        let tab_page = tabs.append(page);
        tab_page.set_title(&tab_title(&page.name(), false));
        self.update_layout();
        if select {
            self.select(page);
        }
        self.refresh_page(page);
    }

    /// The window's hooks on a new tab's view and on its buffer.
    fn install_page(&self, page: &EditorPage) {
        let page = page.clone();
        let buffer = page.buffer();
        buffer.connect_modified_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            #[weak]
            page,
            move |_| window.refresh_page(&page)
        ));
        let refresh_status = glib::clone!(
            #[weak(rename_to = window)]
            self,
            #[weak]
            page,
            move || {
                if window.current_page().as_ref() == Some(&page) {
                    window.update_status();
                }
            }
        );
        let on_cursor = refresh_status.clone();
        buffer.connect_cursor_position_notify(move |_| on_cursor());
        let on_mark = refresh_status.clone();
        buffer.connect_mark_set(move |_, _, mark| {
            if mark.name().as_deref() == Some("selection_bound") {
                on_mark();
            }
        });
        self.connect_page_search(&page);
        let on_language = refresh_status.clone();
        buffer.connect_language_notify(move |_| on_language());
        let on_column = refresh_status.clone();
        page.column_view()
            .connect_column_changed(move |_| on_column());
        page.view()
            .connect_overwrite_notify(move |_| refresh_status());

        // Ctrl+scroll zooms, through the same actions as Ctrl++ and Ctrl+-.
        let scroll = gtk::EventControllerScroll::new(
            gtk::EventControllerScrollFlags::VERTICAL | gtk::EventControllerScrollFlags::DISCRETE,
        );
        scroll.set_propagation_phase(gtk::PropagationPhase::Capture);
        scroll.connect_scroll(glib::clone!(
            #[weak(rename_to = window)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |controller, _, dy| {
                if !controller
                    .current_event_state()
                    .contains(gdk::ModifierType::CONTROL_MASK)
                    || dy == 0.0
                {
                    return glib::Propagation::Proceed;
                }
                let id = if dy < 0.0 {
                    ActionId::ZoomIn
                } else {
                    ActionId::ZoomOut
                };
                let _ = WidgetExt::activate_action(
                    &window.inner.window,
                    &actions::detailed_name(id),
                    None,
                );
                glib::Propagation::Stop
            }
        ));
        page.view().add_controller(scroll);
        self.connect_page_tools(&page);
        self.connect_page_split(&page);
        self.refresh_indentation(&page);
    }

    pub fn new_untitled(&self) -> EditorPage {
        let used = self
            .pages()
            .into_iter()
            .filter_map(|page| page.state().untitled);
        let number = next_untitled_number(used);
        let page = self.add_page(DocState::untitled(number));
        page.view().grab_focus();
        page
    }

    /// Updates the tab label, tooltip, and the window title and status when it is current; for
    /// every tab of the document.
    pub(in crate::window) fn refresh_page(&self, page: &EditorPage) {
        for page in page.document_pages() {
            let Some(tab_page) = self.tab_page(&page) else {
                continue;
            };
            tab_page.set_title(&tab_title(&page.name(), page.is_dirty()));
            let tooltip = page
                .path()
                .map_or_else(|| page.name(), |path| path.display().to_string());
            tab_page.set_tooltip(&glib::markup_escape_text(&tooltip));
            tab_page.set_loading(page.is_loading());
            if tab_page.is_pinned() {
                self.update_pin_icon(&page);
            }
        }
        if self
            .current_page()
            .is_some_and(|current| current.same_document(page))
        {
            self.update_title();
            self.update_status();
        }
    }

    /// The selected tab of view `index` changed.
    pub(in crate::window) fn on_view_selected(&self, index: usize) {
        if self.inner.tabs.is_moving() {
            return;
        }
        // A restored tab loads its text when it is first shown, in either view.
        if let Some(page) = self.inner.views[index].tabs.selected_page() {
            self.session_page_selected(&page_of(&page));
        }
        if index == self.inner.active.get() {
            self.current_changed(true);
        }
    }

    /// Another tab is current: in the active view, or another view became the active one.
    /// With `take_focus` its editor takes the focus, unless the user is in the find bar, the
    /// results panel, a popover or a dialog; switching with Ctrl+Tab from the find field keeps
    /// it.
    pub(in crate::window) fn current_changed(&self, take_focus: bool) {
        // A document in both views has one caret in its buffer: the current view's.
        if let Some(page) = self.current_page() {
            page.take_caret();
        }
        self.update_title();
        self.update_status();
        self.update_actions();
        self.sync_search_highlight();
        self.update_match_count();
        if take_focus {
            self.restore_focus();
        }
        if let Some(page) = self.current_page() {
            self.session_page_selected(&page);
        }
        self.on_nav_page_selected();
    }

    fn update_title(&self) {
        let title = match self.current_page() {
            Some(page) => {
                window_title(&page.name(), page.dir_display().as_deref(), page.is_dirty())
            }
            None => stet_domain::document::APP_NAME.to_owned(),
        };
        self.inner.window.set_title(Some(&title));
    }
}
