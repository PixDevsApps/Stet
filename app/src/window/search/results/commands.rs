//! The window's side of the results panel: its buttons, keys and match colour, the commands
//! that show, step through, copy and clear it, and going to a result.

use super::super::SearchCommand;
use super::{FileTarget, Jump};
use crate::convert::BufferPositions;
use crate::editor::EditorPage;
use crate::editor::load::JUMP_LOAD_WAIT;
use crate::window::{Location, Window};
use gtk4::prelude::*;
use gtk4::{gdk, glib};
use std::path::Path;
use stet_domain::actions::ActionId;

/// A hit chosen in the panel, its keys and buttons, and the match colour of the theme.
pub(in crate::window) fn connect(window: &Window) {
    let results = &window.inner.results;
    results.connect_jump(glib::clone!(
        #[weak]
        window,
        move |jump| window.jump_to(jump)
    ));
    results.keys.connect_key_pressed(glib::clone!(
        #[weak]
        window,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, key, _, state| window.on_results_key(key, state)
    ));
    for (button, command) in [
        (&results.stop, SearchCommand::StopFindInFiles),
        (&results.copy, SearchCommand::CopyResults),
        (&results.clear, SearchCommand::ClearResults),
        (&results.close, SearchCommand::CloseResults),
    ] {
        button.connect_clicked(glib::clone!(
            #[weak]
            window,
            move |_| window.run_search(command)
        ));
    }
    let appearance = &window.inner.shared.appearance;
    appearance.connect_scheme_changed(glib::clone!(
        #[weak]
        window,
        move |scheme| window
            .inner
            .results
            .set_match_color(search_match_color(scheme))
    ));
    if let Some(scheme) = appearance.scheme() {
        results.set_match_color(search_match_color(&scheme));
    }
    if let Some(action) = window.action(ActionId::StopFindInFiles) {
        action.set_enabled(false);
    }
}

impl Window {
    /// Shows the results panel at its last height, optionally with the focus in it.
    pub fn show_results(&self, focus: bool) {
        let results = &self.inner.results;
        if !results.root.is_visible() {
            results.root.set_visible(true);
            let paned = self.inner.layout.paned.clone();
            let height = results.height.get();
            let place = move || {
                let total = paned.height();
                if total > height {
                    paned.set_position(total - height);
                }
            };
            if self.inner.layout.paned.height() > 0 {
                place();
            } else {
                glib::idle_add_local_once(place);
            }
        }
        if focus {
            let position = results.list.model().map_or(0, |model| model.n_items());
            if position > 0 {
                results.list.grab_focus();
            } else {
                results.close.grab_focus();
            }
        }
    }

    pub fn hide_results(&self) {
        let results = &self.inner.results;
        if results.root.is_visible() {
            let paned = &self.inner.layout.paned;
            let height = paned.height() - paned.position();
            if height > 40 {
                results.height.set(height);
            }
            results.root.set_visible(false);
        }
        if let Some(page) = self.current_page() {
            page.view().grab_focus();
        }
    }

    /// Whether the focus is inside the results panel.
    pub fn results_focused(&self) -> bool {
        RootExt::focus(&self.inner.window)
            .is_some_and(|focus| focus.is_ancestor(&self.inner.results.root))
    }

    /// Search Results Window (F7, Ctrl+Alt+R): into the panel, or back to the editor.
    pub(in crate::window::search) fn toggle_results_focus(&self) {
        if self.results_focused() {
            if let Some(page) = self.current_page() {
                page.view().grab_focus();
            }
        } else {
            self.show_results(true);
        }
    }

    /// Next or previous search result (F4, Shift+F4).
    pub(in crate::window::search) fn step_result(&self, forward: bool) {
        match self.inner.results.step(forward) {
            Some(jump) => {
                self.show_results(false);
                self.jump_to(jump);
            }
            None => self.toast("No search results"),
        }
    }

    pub(in crate::window::search) fn copy_results(&self) {
        let text = self.inner.results.text();
        if text.is_empty() {
            self.toast("No search results to copy");
            return;
        }
        self.inner.window.clipboard().set_text(&text);
        self.toast("Copied the search results");
    }

    pub(in crate::window::search) fn clear_results(&self) {
        let results = &self.inner.results;
        results.clear();
        results.set_status_text("");
    }

    /// Goes to a result: selects its tab (opening the file if needed) and the match.
    pub(in crate::window) fn jump_to(&self, jump: Jump) {
        self.note_jump();
        let open_page = match &jump.target {
            FileTarget::Page { page, path } => page
                .upgrade()
                .filter(|page| self.tab_page(page).is_some())
                .or_else(|| self.page_showing(path.as_ref()?)),
            FileTarget::Path(path) => self.page_showing(path),
        };
        if let Some(page) = open_page {
            self.select_result(&page, &jump);
            return;
        }
        let path = match &jump.target {
            FileTarget::Path(path)
            | FileTarget::Page {
                path: Some(path), ..
            } => path.clone(),
            FileTarget::Page { path: None, .. } => {
                self.toast("That document is closed");
                return;
            }
        };
        let opened = self.open_locations(vec![Location {
            path,
            position: Some(jump.position.into()),
            create: false,
        }]);
        let Some(page) = opened.into_iter().next() else {
            return;
        };
        let window = self.clone();
        self.spawn(async move {
            if page.loaded(JUMP_LOAD_WAIT).await && window.tab_page(&page).is_some() {
                window.select_result(&page, &jump);
            }
        });
    }

    fn page_showing(&self, path: &Path) -> Option<EditorPage> {
        self.documents()
            .into_iter()
            .find(|page| page.shows(path, None))
    }

    fn select_result(&self, page: &EditorPage, jump: &Jump) {
        self.select(page);
        let buffer = page.buffer();
        let mut start = buffer.iter_at_line_or_last(jump.position.line);
        for _ in 0..jump.position.column {
            if start.ends_line() {
                break;
            }
            start.forward_char();
        }
        let mut end = start;
        for _ in 0..jump.length {
            if end.ends_line() {
                break;
            }
            end.forward_char();
        }
        page.clear_current_match();
        buffer.select_range(&end, &start);
        page.view()
            .scroll_to_mark(&buffer.get_insert(), 0.1, true, 0.0, 0.3);
        page.view().grab_focus();
        self.update_match_count();
    }

    /// Keys inside the results panel: Escape closes it, Ctrl+C copies the selected rows.
    pub(in crate::window) fn on_results_key(
        &self,
        key: gdk::Key,
        state: gdk::ModifierType,
    ) -> glib::Propagation {
        let control = state.contains(gdk::ModifierType::CONTROL_MASK);
        match key {
            gdk::Key::Escape => {
                self.hide_results();
                glib::Propagation::Stop
            }
            gdk::Key::c | gdk::Key::C | gdk::Key::Insert if control => {
                if let Some(text) = self.inner.results.selected_text() {
                    self.inner.window.clipboard().set_text(&text);
                }
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    }
}

/// The background of the scheme's `search-match` style, which emphasizes matches.
fn search_match_color(scheme: &sourceview5::StyleScheme) -> Option<gdk::RGBA> {
    let style = scheme.style("search-match")?;
    gdk::RGBA::parse(style.background()?.as_str()).ok()
}
