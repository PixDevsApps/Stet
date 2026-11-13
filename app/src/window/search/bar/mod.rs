//! The find bar's widgets: Find, Replace, Find in Files and Mark as rows of one bar, below the
//! editor so the text never shifts. The bar keeps its fields' histories ([`history`]), compiles
//! the find field, and styles the find and replace fields; [`build`] makes its widget tree.
//! The window reads it through typed snapshots ([`FindBar::options`],
//! [`FindBar::files_request`], [`FindBar::mark_options`]) and reacts to it through its
//! `connect_*` callbacks.

mod build;
mod history;

use crate::convert::to_u32;
use crate::editor::search::Compiled;
use crate::runtime::Flag;
use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, glib, pango};
use history::HistoryButton;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use stet_domain::document::expand_home;
use stet_domain::marks::MarkStyle;
use stet_domain::search::report::template_warning;
use stet_domain::search::{
    Query, Recall, SearchHistory, SearchMode, SearchOptions, Template, TranslateError,
};
use stet_infrastructure::search::{Matcher, PatternError};

/// Which command's rows the bar shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarMode {
    Find,
    Replace,
    Files,
    /// The Mark row.
    Mark,
}

/// What a button of the bar asks the window to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarCommand {
    Count,
    FindAllInDocument,
    FindAllInOpenDocuments,
    ReplaceNext,
    ReplaceAll,
    ReplaceAllInOpenDocuments,
    /// Starts the search the Find in Files rows describe.
    FindInFiles,
    ReplaceInFiles,
    MarkAll,
    ClearMarks,
    CopyMarkedText,
}

/// The mode drop-down's entries, in order.
const MODES: [(SearchMode, &str); 3] = [
    (SearchMode::Normal, "Normal"),
    (SearchMode::Extended, "Extended"),
    (SearchMode::Regex, "Regex"),
];

/// The Mark row's styles, in order.
const MARK_STYLES: [(MarkStyle, &str); 6] = [
    (MarkStyle::Mark, "Find Mark Style"),
    (MarkStyle::Token1, "1st Style"),
    (MarkStyle::Token2, "2nd Style"),
    (MarkStyle::Token3, "3rd Style"),
    (MarkStyle::Token4, "4th Style"),
    (MarkStyle::Token5, "5th Style"),
];

/// A message under the find field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Info,
    Error,
}

/// The fields with a history, in [`FindBar::recalls`] order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Find,
    Replace,
    Directory,
    Filters,
}

impl Field {
    const ALL: [Self; 4] = [Self::Find, Self::Replace, Self::Directory, Self::Filters];

    fn index(self) -> usize {
        self as usize
    }
}

/// A key in one of the bar's fields that the window acts on; Up and Down walk the field's
/// history in the bar itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKey {
    /// Enter, or Shift+Enter with `shift`, without Ctrl, Alt or Super.
    Enter {
        field: Field,
        shift: bool,
    },
    Escape,
}

/// The files row as the bar shows it: where Find in Files and Replace in Files look, and at
/// which files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesRequest {
    /// The folder field, trimmed, as typed: `~` is not expanded.
    pub folder: String,
    /// The filters field, trimmed.
    pub filters: String,
    pub subfolders: bool,
    pub hidden: bool,
    /// Leave out what `.gitignore` and `.ignore` files ignore.
    pub gitignore: bool,
}

impl FilesRequest {
    /// The folder, with `~` expanded.
    pub fn folder_path(&self) -> PathBuf {
        expand_home(&self.folder, std::env::home_dir().as_deref())
    }
}

/// The Mark row's choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarkOptions {
    pub style: MarkStyle,
    /// Also bookmark every line with a match.
    pub bookmark_line: bool,
    /// Clear the style's marks (and with `bookmark_line` the bookmarks) first.
    pub purge: bool,
}

/// The find bar: its widgets, its fields' histories and the compiled query.
pub struct FindBar {
    revealer: gtk::Revealer,
    entry: gtk::SearchEntry,
    find_history: HistoryButton,
    case: gtk::ToggleButton,
    word: gtk::ToggleButton,
    in_selection: gtk::ToggleButton,
    mode: gtk::DropDown,
    previous: gtk::Button,
    next: gtk::Button,
    count: gtk::Label,
    message: gtk::Label,
    wrap: gtk::CheckButton,
    backward: gtk::CheckButton,
    dot_newline: gtk::CheckButton,
    count_button: gtk::Button,
    find_all_document: gtk::Button,
    find_all_open: gtk::Button,
    close: gtk::Button,
    replace_row: gtk::Box,
    replace_entry: gtk::Entry,
    replace_history: HistoryButton,
    replace_button: gtk::Button,
    replace_all_button: gtk::Button,
    replace_open_button: gtk::Button,
    files_row: gtk::Box,
    /// Replace in Files, next to Find in Files.
    replace_in_files: gtk::Button,
    directory: gtk::Entry,
    browse: gtk::Button,
    filters: gtk::Entry,
    subfolders: gtk::CheckButton,
    hidden: gtk::CheckButton,
    gitignore: gtk::CheckButton,
    find_in_files: gtk::Button,
    /// Mark: the row, its style, its options and its buttons.
    mark_row: gtk::Box,
    mark_style: gtk::DropDown,
    bookmark_line: gtk::CheckButton,
    purge: gtk::CheckButton,
    mark_all: gtk::Button,
    clear_marks: gtk::Button,
    copy_marked: gtk::Button,
    /// Each field's key handler (Enter, Shift+Enter, Escape, Up, Down), in [`Field`] order;
    /// the self-test drives them.
    keys: [gtk::EventControllerKey; 4],
    bar_mode: Cell<BarMode>,
    compiled: RefCell<Option<Rc<Compiled>>>,
    /// The find field's text when it was last compiled.
    compiled_text: RefCell<String>,
    next_id: Cell<u64>,
    error: RefCell<Option<PatternError>>,
    history: RefCell<SearchHistory>,
    recalls: RefCell<[Recall; 4]>,
    /// The text each field's history walk last put in, so that only other edits end the walk.
    recalled: RefCell<[Option<String>; 4]>,
    recalling: Flag,
}

impl FindBar {
    /// What the bar does by itself: a field's edit ends its history walk, the replace field's
    /// style follows its text and the mode, the mode sets which options apply, and the history
    /// drop-downs list the history as they open.
    fn connect_own(self: &Rc<Self>) {
        for field in Field::ALL {
            self.editable(field).connect_changed(glib::clone!(
                #[weak(rename_to = bar)]
                self,
                move |_| bar.field_changed(field)
            ));
        }
        self.replace_entry.connect_changed(glib::clone!(
            #[weak(rename_to = bar)]
            self,
            move |_| bar.refresh_replace_style()
        ));
        self.mode.connect_selected_notify(glib::clone!(
            #[weak(rename_to = bar)]
            self,
            move |_| {
                let regex = bar.search_mode() == SearchMode::Regex;
                bar.word.set_sensitive(!regex);
                bar.dot_newline.set_sensitive(regex);
                bar.refresh_replace_style();
            }
        ));
        for (history, field) in [
            (&self.find_history, Field::Find),
            (&self.replace_history, Field::Replace),
        ] {
            if let Some(popover) = history.button.popover() {
                popover.connect_show(glib::clone!(
                    #[weak(rename_to = bar)]
                    self,
                    move |_| bar.fill_history(field)
                ));
            }
        }
    }

    /// `on_key` gets Enter and Escape in the bar's fields; Up and Down walk the field's
    /// history.
    pub fn connect_key(self: &Rc<Self>, on_key: impl Fn(FieldKey) + 'static) {
        let on_key = Rc::new(on_key);
        for (keys, field) in self.keys.iter().zip(Field::ALL) {
            let on_key = Rc::clone(&on_key);
            keys.connect_key_pressed(glib::clone!(
                #[weak(rename_to = bar)]
                self,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, key, _, state| {
                    let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
                    let plain = !state.intersects(
                        gdk::ModifierType::CONTROL_MASK
                            | gdk::ModifierType::ALT_MASK
                            | gdk::ModifierType::SUPER_MASK,
                    );
                    match key {
                        gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter if plain => {
                            on_key(FieldKey::Enter { field, shift });
                            glib::Propagation::Stop
                        }
                        gdk::Key::Escape => {
                            on_key(FieldKey::Escape);
                            glib::Propagation::Stop
                        }
                        gdk::Key::Up | gdk::Key::Down if plain && !shift => {
                            bar.recall(field, key == gdk::Key::Up);
                            glib::Propagation::Stop
                        }
                        _ => glib::Propagation::Proceed,
                    }
                }
            ));
        }
    }

    /// `on_change` runs when the query changes: with `true` as the find field's text does
    /// (after the search delay), with `false` as an option that changes the query does.
    pub fn connect_query_changed(&self, on_change: impl Fn(bool) + 'static) {
        let on_change = Rc::new(on_change);
        let changed = Rc::clone(&on_change);
        self.entry.connect_search_changed(move |_| changed(true));
        for button in [&self.case, &self.word] {
            let changed = Rc::clone(&on_change);
            button.connect_toggled(move |_| changed(false));
        }
        let changed = Rc::clone(&on_change);
        self.dot_newline.connect_toggled(move |_| changed(false));
        self.mode.connect_selected_notify(move |_| on_change(false));
    }

    /// `on_toggle` runs when Wrap around is turned on or off.
    pub fn connect_wrap_toggled(&self, on_toggle: impl Fn() + 'static) {
        self.wrap.connect_toggled(move |_| on_toggle());
    }

    /// `on_step` runs with `true` for the Find next button, `false` for Find previous.
    pub fn connect_step(&self, on_step: impl Fn(bool) + 'static) {
        let on_step = Rc::new(on_step);
        for (button, forward) in [(&self.previous, false), (&self.next, true)] {
            let on_step = Rc::clone(&on_step);
            button.connect_clicked(move |_| on_step(forward));
        }
    }

    /// `on_close` runs when the close button is clicked.
    pub fn connect_close(&self, on_close: impl Fn() + 'static) {
        self.close.connect_clicked(move |_| on_close());
    }

    /// `on_browse` runs when the folder button is clicked.
    pub fn connect_browse(&self, on_browse: impl Fn() + 'static) {
        self.browse.connect_clicked(move |_| on_browse());
    }

    /// `on_command` runs with the command of the bar's button that was clicked: Count, Find
    /// All, Replace, Find in Files, Replace in Files or one of Mark's. A button in a popover
    /// closes it first.
    pub fn connect_command(&self, on_command: impl Fn(BarCommand) + 'static) {
        let on_command = Rc::new(on_command);
        for (button, command) in [
            (&self.count_button, BarCommand::Count),
            (&self.find_all_document, BarCommand::FindAllInDocument),
            (&self.find_all_open, BarCommand::FindAllInOpenDocuments),
            (&self.replace_button, BarCommand::ReplaceNext),
            (&self.replace_all_button, BarCommand::ReplaceAll),
            (
                &self.replace_open_button,
                BarCommand::ReplaceAllInOpenDocuments,
            ),
            (&self.find_in_files, BarCommand::FindInFiles),
            (&self.replace_in_files, BarCommand::ReplaceInFiles),
            (&self.mark_all, BarCommand::MarkAll),
            (&self.clear_marks, BarCommand::ClearMarks),
            (&self.copy_marked, BarCommand::CopyMarkedText),
        ] {
            let on_command = Rc::clone(&on_command);
            button.connect_clicked(move |button| {
                if let Some(popover) = button
                    .ancestor(gtk::Popover::static_type())
                    .and_downcast::<gtk::Popover>()
                {
                    popover.popdown();
                }
                on_command(command);
            });
        }
    }

    /// The bar's widget, which the window places below the editor.
    pub fn root(&self) -> &gtk::Revealer {
        &self.revealer
    }

    pub fn is_open(&self) -> bool {
        self.revealer.reveals_child()
    }

    pub fn bar_mode(&self) -> BarMode {
        self.bar_mode.get()
    }

    /// Shows the bar with `mode`'s rows, with the find field focused and its text selected.
    pub fn show(&self, mode: BarMode) {
        self.bar_mode.set(mode);
        // Find in Files has the replace field too, for Replace in Files; the buttons that
        // replace in the document belong to Replace.
        self.replace_row
            .set_visible(matches!(mode, BarMode::Replace | BarMode::Files));
        for button in [
            &self.replace_button,
            &self.replace_all_button,
            &self.replace_open_button,
        ] {
            button.set_visible(mode == BarMode::Replace);
        }
        self.files_row.set_visible(mode == BarMode::Files);
        self.mark_row.set_visible(mode == BarMode::Mark);
        self.revealer.set_reveal_child(true);
        self.entry.grab_focus();
        self.entry.select_region(0, -1);
    }

    pub fn hide(&self) {
        self.revealer.set_reveal_child(false);
    }

    /// The find field's text.
    pub fn query_text(&self) -> String {
        self.entry.text().to_string()
    }

    /// Puts `text` into the find field; it is compiled once the field's search delay is over,
    /// or by [`Self::compile`].
    pub fn set_query_text(&self, text: &str) {
        self.entry.set_text(text);
    }

    pub fn search_mode(&self) -> SearchMode {
        MODES
            .get(self.mode.selected() as usize)
            .map_or(SearchMode::Normal, |(mode, _)| *mode)
    }

    pub fn set_search_mode(&self, mode: SearchMode) {
        if let Some(index) = MODES.iter().position(|(candidate, _)| *candidate == mode) {
            self.mode.set_selected(to_u32(index));
        }
    }

    /// The options as the bar shows them.
    pub fn options(&self) -> SearchOptions {
        let mode = self.search_mode();
        SearchOptions {
            mode,
            match_case: self.case.is_active(),
            whole_word: self.word.is_active(),
            wrap: self.wrap.is_active(),
            backward: self.backward.is_active(),
            in_selection: self.in_selection.is_active(),
            dot_matches_newline: self.dot_newline.is_active(),
        }
    }

    /// The files row's folder, filters and options.
    pub fn files_request(&self) -> FilesRequest {
        FilesRequest {
            folder: self.directory.text().trim().to_owned(),
            filters: self.filters.text().trim().to_owned(),
            subfolders: self.subfolders.is_active(),
            hidden: self.hidden.is_active(),
            gitignore: self.gitignore.is_active(),
        }
    }

    /// Puts `folder` into the folder field.
    pub fn set_folder(&self, folder: &str) {
        self.directory.set_text(folder);
    }

    /// The Mark row's style and options.
    pub fn mark_options(&self) -> MarkOptions {
        MarkOptions {
            style: MARK_STYLES
                .get(self.mark_style.selected() as usize)
                .map_or(MarkStyle::Mark, |(style, _)| *style),
            bookmark_line: self.bookmark_line.is_active(),
            purge: self.purge.is_active(),
        }
    }

    /// The compiled query, while the find field holds a valid one.
    pub fn compiled(&self) -> Option<Rc<Compiled>> {
        self.compiled.borrow().clone()
    }

    /// The compiled query, compiling the find field first if needed; `None`, with the bar
    /// saying why, when the field is empty or invalid.
    pub fn query(&self) -> Option<Rc<Compiled>> {
        if self.compiled().is_none() {
            self.compile();
        }
        let compiled = self.compiled();
        if compiled.is_none() && self.pattern_error().is_none() {
            self.set_message("Type what to search for", Tone::Info);
        }
        compiled
    }

    pub fn pattern_error(&self) -> Option<PatternError> {
        self.error.borrow().clone()
    }

    fn editable(&self, field: Field) -> gtk::Editable {
        match field {
            Field::Find => self.entry.clone().upcast(),
            Field::Replace => self.replace_entry.clone().upcast(),
            Field::Directory => self.directory.clone().upcast(),
            Field::Filters => self.filters.clone().upcast(),
        }
    }

    pub fn set_message(&self, text: &str, tone: Tone) {
        self.message.set_label(text);
        self.message
            .set_tooltip_text((!text.is_empty()).then_some(text));
        if tone == Tone::Error {
            self.message.add_css_class("error");
        } else {
            self.message.remove_css_class("error");
        }
    }

    /// Shows the match count next to the arrows.
    pub fn set_count(&self, label: &str) {
        self.count.set_label(label);
    }

    /// Shows a pattern error on the find field without taking the focus: the error style, the
    /// message as tooltip, and a wavy underline under the character it is about.
    fn show_error(&self, error: Option<&PatternError>) {
        self.error.replace(error.cloned());
        let text = self.entry.delegate().and_downcast::<gtk::Text>();
        match error {
            None => {
                self.entry.remove_css_class("error");
                self.entry.set_tooltip_text(None);
                if let Some(text) = text {
                    text.set_attributes(None);
                }
            }
            Some(error) => {
                self.entry.add_css_class("error");
                let message = format!("{} (at character {})", error.message, error.offset + 1);
                self.entry.set_tooltip_text(Some(&message));
                self.set_message(&message, Tone::Error);
                if let Some(text) = text {
                    let pattern = self.entry.text();
                    let chars = pattern.chars().count();
                    let at = error.offset.min(chars.saturating_sub(1));
                    let start = pattern
                        .char_indices()
                        .nth(at)
                        .map_or(pattern.len(), |(byte, _)| byte);
                    let end = pattern
                        .char_indices()
                        .nth(at + 1)
                        .map_or(pattern.len(), |(byte, _)| byte);
                    let attributes = pango::AttrList::new();
                    let mut underline = pango::AttrInt::new_underline(pango::Underline::Error);
                    underline.set_start_index(to_u32(start));
                    underline.set_end_index(to_u32(end.max(start + 1)));
                    attributes.insert(underline);
                    text.set_attributes(Some(&attributes));
                }
            }
        }
    }

    /// Compiles the find field with the options. Returns the new query, if valid.
    pub fn compile(&self) -> Option<Rc<Compiled>> {
        let pattern = self.entry.text().to_string();
        self.compiled_text.replace(pattern.clone());
        let compiled = if pattern.is_empty() {
            self.show_error(None);
            None
        } else {
            let query = Query::new(pattern, self.options());
            match Matcher::new(&query) {
                Ok(matcher) => {
                    self.show_error(None);
                    self.next_id.set(self.next_id.get() + 1);
                    Some(Rc::new(Compiled {
                        id: self.next_id.get(),
                        query,
                        matcher: Arc::new(matcher),
                    }))
                }
                Err(error) => {
                    self.show_error(Some(&error));
                    None
                }
            }
        };
        self.compiled.replace(compiled.clone());
        compiled
    }

    /// The replace field's text.
    pub fn replace_text(&self) -> String {
        self.replace_entry.text().to_string()
    }

    /// The replace field read in the current mode, or why no text can hold it.
    pub fn replace_template(&self) -> Result<Template, TranslateError> {
        Template::for_mode(self.search_mode(), &self.replace_entry.text())
            .map_err(TranslateError::from)
    }

    /// Styles the replace field for its template in the current mode: the error style for a
    /// template no text can hold, the warning style with an explanation for one that is easy
    /// to get wrong.
    fn refresh_replace_style(&self) {
        let entry = &self.replace_entry;
        let (tooltip, class) = match Template::for_mode(self.search_mode(), &entry.text()) {
            Ok(template) if template.warnings().is_empty() => (None, None),
            Ok(template) => {
                let warnings: Vec<String> = template
                    .warnings()
                    .iter()
                    .map(|warning| template_warning(*warning))
                    .collect();
                (Some(warnings.join("\n")), Some("warning"))
            }
            Err(error) => (Some(TranslateError::from(error).to_string()), Some("error")),
        };
        for style in ["error", "warning"] {
            if Some(style) == class {
                entry.add_css_class(style);
            } else {
                entry.remove_css_class(style);
            }
        }
        entry.set_tooltip_text(tooltip.as_deref());
    }
}

/// What the self-test reads and drives on the bar.
#[cfg(feature = "selftest")]
impl FindBar {
    /// The text field named `name`: find, replace, directory or filters.
    pub fn field(&self, name: &str) -> Option<gtk::Editable> {
        let field = match name {
            "find" => Field::Find,
            "replace" => Field::Replace,
            "directory" => Field::Directory,
            "filters" => Field::Filters,
            _ => return None,
        };
        Some(self.editable(field))
    }

    /// Sets the option named `name` (case, word, selection, wrap, backward, dot-newline,
    /// subfolders, hidden, gitignore, bookmark or purge); false if there is no such option.
    pub fn set_option(&self, name: &str, active: bool) -> bool {
        let button: gtk::Widget = match name {
            "case" => self.case.clone().upcast(),
            "word" => self.word.clone().upcast(),
            "selection" => self.in_selection.clone().upcast(),
            "wrap" => self.wrap.clone().upcast(),
            "backward" => self.backward.clone().upcast(),
            "dot-newline" => self.dot_newline.clone().upcast(),
            "subfolders" => self.subfolders.clone().upcast(),
            "hidden" => self.hidden.clone().upcast(),
            "gitignore" => self.gitignore.clone().upcast(),
            "bookmark" => self.bookmark_line.clone().upcast(),
            "purge" => self.purge.clone().upcast(),
            _ => return false,
        };
        if let Some(toggle) = button.downcast_ref::<gtk::ToggleButton>() {
            toggle.set_active(active);
        } else if let Some(check) = button.downcast_ref::<gtk::CheckButton>() {
            check.set_active(active);
        }
        true
    }

    /// The key handler of the field named `name`: find, replace, directory or filters.
    pub fn key_controller(&self, name: &str) -> Option<gtk::EventControllerKey> {
        let field = match name {
            "find" => Field::Find,
            "replace" => Field::Replace,
            "directory" => Field::Directory,
            "filters" => Field::Filters,
            _ => return None,
        };
        Some(self.keys[field.index()].clone())
    }

    /// The match count.
    pub fn count_text(&self) -> String {
        self.count.label().to_string()
    }

    /// The message, and whether it shows as an error.
    pub fn message(&self) -> (String, bool) {
        (
            self.message.label().to_string(),
            self.message.has_css_class("error"),
        )
    }

    /// The find field's text when it was last compiled.
    pub fn compiled_text(&self) -> String {
        self.compiled_text.borrow().clone()
    }

    pub fn set_mark_style(&self, style: MarkStyle) {
        if let Some(index) = MARK_STYLES.iter().position(|(each, _)| *each == style) {
            self.mark_style.set_selected(to_u32(index));
        }
    }

    /// Clicks the Find in Files button.
    pub fn click_find_in_files(&self) {
        self.find_in_files.emit_clicked();
    }

    /// Opens the find or replace field's history drop-down and picks entry `index` as a click
    /// would; `None` if there is no drop-down `name`.
    pub fn pick_history(&self, name: &str, index: i32) -> Option<bool> {
        let history = match name {
            "find" => &self.find_history,
            "replace" => &self.replace_history,
            _ => return None,
        };
        history.button.popup();
        Some(match history.list.row_at_index(index) {
            Some(row) if row.is_activatable() => {
                row.emit_activate();
                true
            }
            _ => false,
        })
    }
}
