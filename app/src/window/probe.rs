//! What the self-test harness (`stet --self-test`) reads and drives in the window. The harness
//! reaches the window only through these methods, never through its fields, so the window's
//! inside can change without the harness noticing. They return plain values or clones of the
//! widgets the harness acts on; widgets are named by the same words the test scripts use.
//!
//! Probes that need a feature's private state live with that feature, in a
//! `#[cfg(feature = "selftest")]` block at the end of its module.

use super::quick_open::QuickOpenStats;
use super::toolbox::ToolStats;
use super::{BarMode, IndentChoice, PickerMode, ReplaceStats, Shared, ViewPane, Window};
use crate::page_search::Compiled;
use gtk4 as gtk;
use gtk4::{gdk, gio, glib};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::ffi::OsString;
use std::rc::Rc;
use stet_domain::marks::MarkStyle;
use stet_domain::search::SearchMode;
use stet_infrastructure::search::PatternError;

impl Shared {
    /// Records desktop commands (a terminal, the file manager) instead of running them.
    pub fn record_launches(&self) {
        self.launches.replace(Some(Vec::new()));
    }

    /// The commands recorded since [`Self::record_launches`].
    pub fn launches(&self) -> Vec<Vec<OsString>> {
        self.launches.borrow().clone().unwrap_or_default()
    }
}

impl ViewPane {
    /// The strip's visible tabs end here, in its coordinates.
    fn tabs_end(&self) -> Option<f32> {
        let scroller = self.tab_scroller.as_ref()?;
        let bounds = scroller.compute_bounds(&self.bar)?;
        Some(bounds.x() + bounds.width())
    }
}

impl Window {
    // ----- windows, views and tabs ----------------------------------------------------------

    pub fn title(&self) -> String {
        self.inner
            .window
            .title()
            .map(|title| title.to_string())
            .unwrap_or_default()
    }

    /// Tab `number` (from 1) across the views, with its view's tab view.
    pub fn tab_at(&self, number: usize) -> Option<(adw::TabView, adw::TabPage)> {
        let mut left = number.checked_sub(1)?;
        for view in &self.inner.views {
            let count = usize::try_from(view.tabs.n_pages()).unwrap_or(0);
            if left < count {
                return Some((view.tabs.clone(), view.tabs.nth_page(left as i32)));
            }
            left -= count;
        }
        None
    }

    /// View `index`'s tab view.
    pub fn tab_view(&self, index: usize) -> adw::TabView {
        self.inner.views[index].tabs.clone()
    }

    /// Both views' tab views.
    pub fn tab_views(&self) -> Vec<adw::TabView> {
        self.inner
            .views
            .iter()
            .map(|view| view.tabs.clone())
            .collect()
    }

    /// View `index`'s tab strip.
    pub fn tab_bar(&self, index: usize) -> adw::TabBar {
        self.inner.views[index].bar.clone()
    }

    /// Where view `index`'s visible tabs end, in its strip's coordinates; `None` if the strip
    /// is built differently than expected.
    pub fn tabs_end(&self, index: usize) -> Option<f32> {
        self.inner.views[index].tabs_end()
    }

    /// The + after view `index`'s last tab, which opens a new tab there.
    pub fn new_tab_button(&self, index: usize) -> &gtk::Button {
        &self.inner.views[index].new_tab
    }

    /// What takes a tab dropped after view `index`'s last tab.
    pub fn tab_drop_target(&self, index: usize) -> &gtk::DropTarget {
        &self.inner.views[index].tab_drop
    }

    /// The tabs of view `index`.
    pub fn view_pages(&self, index: usize) -> Vec<crate::editor::EditorPage> {
        self.inner
            .views
            .get(index)
            .map(|view| super::view_pages(&view.tabs))
            .unwrap_or_default()
    }

    /// The tab menu of view `index`: (label, action) of every item.
    pub fn tab_menu_items(&self, index: usize) -> Vec<(String, String)> {
        let Some(model) = self.inner.views[index].tabs.menu_model() else {
            return Vec::new();
        };
        let mut items = Vec::new();
        menu_items(&model, &mut items);
        items
    }

    pub fn closed_count(&self) -> usize {
        self.inner.closed.borrow().len()
    }

    // ----- the menu, popovers and dialogs ---------------------------------------------------

    pub fn menu_model(&self) -> Option<gio::MenuModel> {
        self.ensure_menu();
        self.inner.menu_button.menu_model()
    }

    /// The hamburger menu's button.
    pub fn menu_button(&self) -> &gtk::MenuButton {
        &self.inner.menu_button
    }

    /// The view whose tab strip has the menu at its start.
    pub fn menu_view(&self) -> Option<usize> {
        let start = self.inner.start.upcast_ref::<gtk::Widget>();
        self.inner
            .views
            .iter()
            .position(|view| view.bar.start_action_widget().as_ref() == Some(start))
    }

    /// The popover named `name`: palette, language, encoding, indentation or quick-open.
    pub fn popover(&self, name: &str) -> Option<gtk::Popover> {
        let inner = &self.inner;
        Some(match name {
            "palette" => inner.palette.popover.clone(),
            "language" => inner.languages.popover.clone(),
            "encoding" => inner.encodings.popover.clone(),
            "indentation" => inner.indentation.popover.clone(),
            "quick-open" => inner.nav.quick_open.popover.clone(),
            _ => return None,
        })
    }

    /// The widget that has the keyboard focus when `name` has it: the find, replace and
    /// results fields, or one of the [popovers](Self::popover).
    pub fn focus_target(&self, name: &str) -> Option<gtk::Widget> {
        let inner = &self.inner;
        Some(match name {
            "find" => inner.find.entry.clone().upcast(),
            "replace" => inner.find.replace_entry.clone().upcast(),
            "results" => inner.results.root.clone().upcast(),
            popover => self.popover(popover)?.upcast(),
        })
    }

    /// The status bar's button named `name`: language, eol, encoding or indentation.
    pub fn status_button(&self, name: &str) -> Option<gtk::MenuButton> {
        let status = &self.inner.status;
        Some(match name {
            "language" => status.language.clone(),
            "eol" => status.eol.clone(),
            "encoding" => status.encoding.clone(),
            "indentation" => status.indentation.clone(),
            _ => return None,
        })
    }

    /// Every item of the status bar, as text.
    pub fn status_text(&self) -> String {
        self.inner.status.text()
    }

    /// The status bar's foreground colour.
    pub fn status_color(&self) -> gdk::RGBA {
        self.inner.status.root.color()
    }

    /// The alert dialog on screen, if any.
    pub fn open_dialog(&self) -> Option<adw::AlertDialog> {
        self.inner.dialog.borrow().clone()
    }

    /// The text of the last toast.
    pub fn last_toast(&self) -> String {
        self.inner.last_toast.borrow().clone()
    }

    pub fn close_palette(&self) {
        self.inner.palette.popover.popdown();
    }

    /// Runs the command palette's selected row, as Enter does.
    pub fn palette_activate(&self) {
        self.inner.palette.activate_selected();
    }

    /// The labels of the command palette's rows, in order.
    pub fn palette_labels(&self) -> Vec<String> {
        self.inner.palette.shown_labels()
    }

    /// Chooses the first language matching `query` in the language picker, as a click would.
    pub fn pick_language(&self, query: &str) -> bool {
        self.inner.languages.choose_matching(query)
    }

    /// Chooses the encoding `id` in the encoding picker in `mode`, as a click would.
    pub fn pick_encoding(&self, mode: PickerMode, id: &str) -> bool {
        self.inner.encodings.choose_id(mode, id)
    }

    /// Chooses `choice` in the indentation picker, as a click would.
    pub fn pick_indentation(&self, choice: IndentChoice) {
        self.inner.indentation.choose(choice);
    }

    /// What the last text tool cost.
    pub fn tool_stats(&self) -> Option<ToolStats> {
        *self.inner.tools.stats.borrow()
    }

    // ----- the find bar ---------------------------------------------------------------------

    pub fn find_open(&self) -> bool {
        self.inner.find.is_open()
    }

    /// The text field named `name`: find, replace, directory or filters on the find bar, or
    /// quick-open.
    pub fn field(&self, name: &str) -> Option<gtk::Editable> {
        let find = &self.inner.find;
        Some(match name {
            "find" => find.entry.clone().upcast(),
            "replace" => find.replace_entry.clone().upcast(),
            "directory" => find.directory.clone().upcast(),
            "filters" => find.filters.clone().upcast(),
            "quick-open" => self.inner.nav.quick_open.entry.clone().upcast(),
            _ => return None,
        })
    }

    /// Sets the find bar's option named `name` (case, word, selection, wrap, backward,
    /// dot-newline, subfolders, hidden, gitignore, bookmark or purge); false if there is no
    /// such option.
    pub fn set_find_option(&self, name: &str, active: bool) -> bool {
        let find = &self.inner.find;
        let button: gtk::Widget = match name {
            "case" => find.case.clone().upcast(),
            "word" => find.word.clone().upcast(),
            "selection" => find.in_selection.clone().upcast(),
            "wrap" => find.wrap.clone().upcast(),
            "backward" => find.backward.clone().upcast(),
            "dot-newline" => find.dot_newline.clone().upcast(),
            "subfolders" => find.subfolders.clone().upcast(),
            "hidden" => find.hidden.clone().upcast(),
            "gitignore" => find.gitignore.clone().upcast(),
            "bookmark" => find.bookmark_line.clone().upcast(),
            "purge" => find.purge.clone().upcast(),
            _ => return false,
        };
        if let Some(toggle) = button.downcast_ref::<gtk::ToggleButton>() {
            toggle.set_active(active);
        } else if let Some(check) = button.downcast_ref::<gtk::CheckButton>() {
            check.set_active(active);
        }
        true
    }

    /// The key handler of the field named `name`: find, replace, directory, filters or
    /// results.
    pub fn key_controller(&self, name: &str) -> Option<gtk::EventControllerKey> {
        let find = &self.inner.find;
        Some(match name {
            "find" => find.keys.clone(),
            "replace" => find.replace_keys.clone(),
            "directory" => find.directory_keys.clone(),
            "filters" => find.filters_keys.clone(),
            "results" => self.inner.results.keys.clone(),
            _ => return None,
        })
    }

    /// The window's key handler for tab switching (Ctrl+Tab).
    pub fn window_keys(&self) -> gtk::EventControllerKey {
        self.inner.nav.keys.clone()
    }

    /// The find bar's match count.
    pub fn find_count(&self) -> String {
        self.inner.find.count.label().to_string()
    }

    /// The find bar's message, and whether it shows as an error.
    pub fn find_message(&self) -> (String, bool) {
        let message = &self.inner.find.message;
        (message.label().to_string(), message.has_css_class("error"))
    }

    pub fn clear_find_message(&self) {
        self.inner.find.set_message("", super::Tone::Info);
    }

    /// The find field's text when it was last compiled.
    pub fn find_compiled_text(&self) -> String {
        self.inner.find.compiled_text()
    }

    /// The compiled query, while the find field holds a valid one.
    pub fn find_compiled(&self) -> Option<Rc<Compiled>> {
        self.inner.find.compiled()
    }

    pub fn find_pattern_error(&self) -> Option<PatternError> {
        self.inner.find.pattern_error()
    }

    pub fn find_bar_mode(&self) -> BarMode {
        self.inner.find.bar_mode()
    }

    pub fn set_find_search_mode(&self, mode: SearchMode) {
        self.inner.find.set_search_mode(mode);
    }

    pub fn set_mark_style(&self, style: MarkStyle) {
        self.inner.find.set_mark_style(style);
    }

    /// Clicks the find bar's Find in Files button.
    pub fn click_find_in_files(&self) {
        self.inner.find.find_in_files.emit_clicked();
    }

    /// Picks entry `index` of the find or replace field's history drop-down, as a click
    /// would; `None` if there is no drop-down `name`.
    pub fn pick_history(&self, name: &str, index: i32) -> Option<bool> {
        let find = &self.inner.find;
        let button = match name {
            "find" => &find.find_history,
            "replace" => &find.replace_history,
            _ => return None,
        };
        Some(button.pick(index))
    }

    /// What the last Replace All cost.
    pub fn replace_stats(&self) -> Option<ReplaceStats> {
        *self.inner.find.stats.borrow()
    }

    /// Forgets the last Replace All's cost.
    pub fn reset_replace_stats(&self) {
        self.inner.find.stats.replace(None);
    }

    // ----- the results panel ----------------------------------------------------------------

    pub fn results_shown(&self) -> bool {
        self.inner.results.is_shown()
    }

    /// The text of the results panel's rows, one per line.
    pub fn results_text(&self) -> String {
        self.inner.results.text()
    }

    /// The results panel's status line.
    pub fn results_status(&self) -> String {
        self.inner.results.status.label().to_string()
    }

    /// The heading of the newest search in the results panel.
    pub fn results_heading(&self) -> Option<String> {
        self.inner.results.latest().map(|search| search.heading())
    }

    pub fn results_search_count(&self) -> u32 {
        self.inner.results.search_count()
    }

    /// Selects the row at `position` and activates it, as Enter does.
    pub fn activate_result(&self, position: u32) -> bool {
        let results = &self.inner.results;
        results.select_position(position) && results.activate_selected()
    }

    // ----- quick open and the switcher ------------------------------------------------------

    pub fn quick_open_shown(&self) -> bool {
        self.inner.nav.quick_open.popover.is_visible()
    }

    /// Selects quick open's row `index`, as Down and Up do.
    pub fn quick_open_select(&self, index: usize) -> Result<(), String> {
        self.inner.nav.quick_open.select_row(index)
    }

    /// Runs quick open's selected row, as Enter does.
    pub fn quick_open_activate(&self) {
        self.inner.nav.quick_open.activate_selected();
    }

    /// The walk is over and the ranking for the field's text is on screen.
    pub fn quick_open_settled(&self) -> bool {
        self.inner.nav.quick_open.is_settled()
    }

    pub fn quick_open_stats(&self) -> QuickOpenStats {
        self.inner.nav.quick_open.stats()
    }

    /// Quick open's rows, as their text.
    pub fn quick_open_labels(&self) -> Vec<String> {
        self.inner.nav.quick_open.shown_labels()
    }

    /// The matched characters of quick open's row `index`.
    pub fn quick_open_positions(&self, index: usize) -> Option<Vec<usize>> {
        self.inner.nav.quick_open.shown_positions(index)
    }

    /// The note under quick open's rows.
    pub fn quick_open_note(&self) -> String {
        self.inner.nav.quick_open.note.label().to_string()
    }

    pub fn switcher_shown(&self) -> bool {
        self.inner.nav.switcher.is_shown()
    }

    /// The tab names in the switcher, and the selected one.
    pub fn switcher_rows(&self) -> (Vec<String>, Option<String>) {
        self.inner.nav.switcher.shown()
    }
}

/// The (label, action) of every item of `model` and its sections.
fn menu_items(model: &gio::MenuModel, out: &mut Vec<(String, String)>) {
    let text = |index: i32, attribute: &str| {
        model
            .item_attribute_value(index, attribute, Some(glib::VariantTy::STRING))
            .and_then(|value| value.get::<String>())
            .unwrap_or_default()
    };
    for index in 0..model.n_items() {
        let action = text(index, "action");
        if !action.is_empty() {
            out.push((text(index, "label"), action));
        }
        if let Some(section) = model.item_link(index, "section") {
            menu_items(&section, out);
        }
    }
}
