//! The find bar (Ctrl+F, Ctrl+H, Ctrl+Shift+F, Ctrl+M): Find, Replace, Find in Files and Mark
//! as rows of one bar, below the editor so the text never shifts.
//!
//! Every query is translated once ([`stet_domain::search::translate`]) and compiled by our own
//! matcher, which also reports pattern errors at the typed character. GtkSourceView's
//! `SearchContext` counts, highlights and finds the queries it can run; our matcher does it
//! for the rest (empty matches, `\K`, `\G`), on a snapshot on a worker, and highlights them
//! with a tag in the scheme's search-match colours. Nothing here replaces text through
//! GtkSourceView (ADR-005).

use super::Window;
use crate::editor::EditorPage;
use crate::page_search::{Compiled, OwnMatches};
use crate::worker;
use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, glib, pango};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use stet_domain::marks::MarkStyle;
use stet_domain::search::report::template_warning;
use stet_domain::search::{
    History, Query, Recall, SearchHistory, SearchMode, SearchOptions, TranslateError,
};
use stet_infrastructure::search::{Matcher, PatternError};

/// Matches our own matcher collects for counting and highlighting in one document.
pub const OWN_MATCH_LIMIT: usize = 1_000_000;
/// How long after the last edit our own matcher searches again.
const OWN_REFRESH_DELAY: Duration = Duration::from_millis(150);
/// The same in large-file mode, where taking the snapshot itself is noticeable.
const OWN_REFRESH_DELAY_LARGE: Duration = Duration::from_millis(1_000);

/// Which command's rows the bar shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarMode {
    Find,
    Replace,
    Files,
    /// The Mark row (M7).
    Mark,
}

/// The mode drop-down's entries, in order.
const MODES: [(SearchMode, &str); 3] = [
    (SearchMode::Normal, "Normal"),
    (SearchMode::Extended, "Extended"),
    (SearchMode::Regex, "Regex"),
];

/// The Mark row's styles, in order (M7).
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

/// Called with the history entry the user picked.
type OnChosen = Rc<dyn Fn(String)>;

/// A history drop-down next to an entry.
pub struct HistoryButton {
    pub button: gtk::MenuButton,
    list: gtk::ListBox,
    entries: Rc<RefCell<Vec<String>>>,
    chosen: Rc<RefCell<Option<OnChosen>>>,
}

impl HistoryButton {
    fn new(tooltip: &str) -> Self {
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .activate_on_single_click(true)
            .build();
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(320)
            .build();
        let popover = gtk::Popover::builder()
            .child(&scrolled)
            .has_arrow(false)
            .build();
        popover.add_css_class("stet-history");
        let button = gtk::MenuButton::builder()
            .icon_name("pan-down-symbolic")
            .tooltip_text(tooltip)
            .popover(&popover)
            .build();
        button.add_css_class("flat");
        let entries: Rc<RefCell<Vec<String>>> = Rc::default();
        let chosen: Rc<RefCell<Option<OnChosen>>> = Rc::default();
        list.connect_row_activated(glib::clone!(
            #[strong]
            entries,
            #[strong]
            chosen,
            #[weak]
            popover,
            move |_, row| {
                let entry = entries.borrow().get(row.index() as usize).cloned();
                let chosen = chosen.borrow().clone();
                if let (Some(entry), Some(chosen)) = (entry, chosen) {
                    popover.popdown();
                    chosen(entry);
                }
            }
        ));
        Self {
            button,
            list,
            entries,
            chosen,
        }
    }

    /// Opens the drop-down and picks entry `index` as a click would, for the self-test.
    pub fn pick(&self, index: i32) -> bool {
        self.button.popup();
        match self.list.row_at_index(index) {
            Some(row) if row.is_activatable() => {
                row.emit_activate();
                true
            }
            _ => false,
        }
    }

    /// Lists `entries`; `chosen` gets the one picked with a click or Enter.
    fn fill(&self, entries: &[String], chosen: OnChosen) {
        self.list.remove_all();
        self.entries.replace(entries.to_vec());
        self.chosen.replace(Some(chosen));
        if entries.is_empty() {
            let empty = gtk::Label::builder()
                .label("No history yet")
                .css_classes(["dim-label"])
                .build();
            self.list.append(
                &gtk::ListBoxRow::builder()
                    .child(&empty)
                    .activatable(false)
                    .build(),
            );
            return;
        }
        for entry in entries {
            let shown: String = entry
                .chars()
                .take(120)
                .map(|c| match c {
                    '\n' => '⏎',
                    '\t' => '⇥',
                    c => c,
                })
                .collect();
            let label = gtk::Label::builder()
                .label(&shown)
                .xalign(0.0)
                .ellipsize(pango::EllipsizeMode::End)
                .build();
            self.list.append(
                &gtk::ListBoxRow::builder()
                    .child(&label)
                    .activatable(true)
                    .build(),
            );
        }
    }
}

pub struct FindBar {
    pub revealer: gtk::Revealer,
    pub entry: gtk::SearchEntry,
    pub find_history: HistoryButton,
    pub case: gtk::ToggleButton,
    pub word: gtk::ToggleButton,
    pub in_selection: gtk::ToggleButton,
    pub mode: gtk::DropDown,
    pub previous: gtk::Button,
    pub next: gtk::Button,
    pub count: gtk::Label,
    pub message: gtk::Label,
    pub wrap: gtk::CheckButton,
    pub backward: gtk::CheckButton,
    pub dot_newline: gtk::CheckButton,
    pub count_button: gtk::Button,
    pub find_all_document: gtk::Button,
    pub find_all_open: gtk::Button,
    pub close: gtk::Button,
    pub replace_row: gtk::Box,
    pub replace_entry: gtk::Entry,
    pub replace_history: HistoryButton,
    pub replace_button: gtk::Button,
    pub replace_all_button: gtk::Button,
    pub replace_open_button: gtk::Button,
    pub files_row: gtk::Box,
    /// Replace in Files (M8), next to Find in Files.
    pub replace_in_files: gtk::Button,
    pub directory: gtk::Entry,
    pub browse: gtk::Button,
    pub filters: gtk::Entry,
    pub subfolders: gtk::CheckButton,
    pub hidden: gtk::CheckButton,
    pub gitignore: gtk::CheckButton,
    pub find_in_files: gtk::Button,
    /// Mark (M7): the row, its style, its options and its buttons.
    pub mark_row: gtk::Box,
    pub mark_style: gtk::DropDown,
    pub bookmark_line: gtk::CheckButton,
    pub purge: gtk::CheckButton,
    pub mark_all: gtk::Button,
    pub clear_marks: gtk::Button,
    pub copy_marked: gtk::Button,
    /// The find field's key handler (Enter, Shift+Enter, Escape, Up, Down); the self-test
    /// drives it, and the other fields' handlers below.
    pub keys: gtk::EventControllerKey,
    pub replace_keys: gtk::EventControllerKey,
    pub directory_keys: gtk::EventControllerKey,
    pub filters_keys: gtk::EventControllerKey,
    /// What the last Replace All cost.
    pub stats: RefCell<Option<super::ReplaceStats>>,
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
    recalling: Cell<bool>,
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
    fn index(self) -> usize {
        self as usize
    }
}

fn toggle(label: &str, tooltip: &str) -> gtk::ToggleButton {
    let button = gtk::ToggleButton::builder()
        .label(label)
        .tooltip_text(tooltip)
        .build();
    button.add_css_class("flat");
    button
}

fn icon_button(icon: &str, tooltip: &str) -> gtk::Button {
    let button = gtk::Button::builder()
        .icon_name(icon)
        .tooltip_text(tooltip)
        .build();
    button.add_css_class("flat");
    button
}

fn text_button(label: &str, tooltip: &str) -> gtk::Button {
    gtk::Button::builder()
        .label(label)
        .tooltip_text(tooltip)
        .build()
}

fn row(widgets: &[&gtk::Widget]) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    row.add_css_class("stet-findbar-row");
    for widget in widgets {
        row.append(*widget);
    }
    row
}

fn key_controller(widget: &impl IsA<gtk::Widget>) -> gtk::EventControllerKey {
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    widget.add_controller(keys.clone());
    keys
}

impl FindBar {
    pub fn new() -> Self {
        let entry = gtk::SearchEntry::builder()
            .placeholder_text("Find")
            .search_delay(80)
            .hexpand(true)
            .build();
        // A placeholder is no accessible name: screen readers announced these fields unnamed.
        entry.update_property(&[gtk::accessible::Property::Label("Find")]);
        let find_history = HistoryButton::new("Find history (↑ and ↓ in the field)");
        let case = toggle("Aa", "Match case");
        let word = toggle("W", "Match whole word only (not in Regex mode)");
        let in_selection = toggle("Sel", "In selection: Count and Replace All stay inside it");
        let mode = gtk::DropDown::from_strings(&MODES.map(|(_, label)| label));
        mode.set_tooltip_text(Some(
            "Search mode: Normal text, Extended (\\n \\r \\t \\0 \\x…) or a regular expression",
        ));
        let previous = icon_button(
            "go-up-symbolic",
            "Find previous (Shift+Enter, Alt+↑, Shift+F3)",
        );
        let next = icon_button("go-down-symbolic", "Find next (Enter, Alt+↓, F3)");
        let count = gtk::Label::builder()
            .css_classes(["dim-label", "stet-count"])
            .xalign(0.0)
            .build();
        let message = gtk::Label::builder()
            .css_classes(["stet-find-message"])
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(pango::EllipsizeMode::End)
            .build();

        let wrap = gtk::CheckButton::builder()
            .label("Wrap around")
            .active(true)
            .build();
        let backward = gtk::CheckButton::builder()
            .label("Backward direction")
            .build();
        let dot_newline = gtk::CheckButton::builder()
            .label(". matches newline (Regex)")
            .sensitive(false)
            .build();
        let menu_button = |label: &str| {
            let button = gtk::Button::builder().label(label).build();
            button.add_css_class("flat");
            if let Some(child) = button.child().and_downcast::<gtk::Label>() {
                child.set_xalign(0.0);
            }
            button
        };
        let count_button = menu_button("Count");
        let find_all_document = menu_button("Find All in Current Document");
        let find_all_open = menu_button("Find All in Open Documents");
        let options = gtk::Box::new(gtk::Orientation::Vertical, 2);
        options.add_css_class("stet-find-options");
        for widget in [
            wrap.upcast_ref::<gtk::Widget>(),
            backward.upcast_ref(),
            dot_newline.upcast_ref(),
        ] {
            options.append(widget);
        }
        options.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        for widget in [&count_button, &find_all_document, &find_all_open] {
            options.append(widget);
        }
        let more = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .tooltip_text("More options, Count and Find All")
            .popover(&gtk::Popover::builder().child(&options).build())
            .build();
        more.add_css_class("flat");
        let close = icon_button("window-close-symbolic", "Close (Escape)");

        let find_row = row(&[
            entry.upcast_ref(),
            find_history.button.upcast_ref(),
            case.upcast_ref(),
            word.upcast_ref(),
            in_selection.upcast_ref(),
            mode.upcast_ref(),
            previous.upcast_ref(),
            next.upcast_ref(),
            count.upcast_ref(),
            message.upcast_ref(),
            more.upcast_ref(),
            close.upcast_ref(),
        ]);

        let replace_entry = gtk::Entry::builder()
            .placeholder_text("Replace with")
            .hexpand(true)
            .build();
        replace_entry.update_property(&[gtk::accessible::Property::Label("Replace with")]);
        let replace_history = HistoryButton::new("Replace history (↑ and ↓ in the field)");
        let replace_button = text_button(
            "Replace",
            "Replace the match and find the next one (Enter in this field)",
        );
        let replace_all_button = text_button("Replace All", "Replace every match");
        let replace_open_button = text_button(
            "Replace All in Open Documents",
            "Replace every match in every open document",
        );
        let replace_row = row(&[
            replace_entry.upcast_ref(),
            replace_history.button.upcast_ref(),
            replace_button.upcast_ref(),
            replace_all_button.upcast_ref(),
            replace_open_button.upcast_ref(),
        ]);
        replace_row.set_visible(false);

        let directory = gtk::Entry::builder()
            .placeholder_text("Folder")
            .hexpand(true)
            .build();
        directory.update_property(&[gtk::accessible::Property::Label("Folder")]);
        let browse = icon_button("folder-open-symbolic", "Choose the folder…");
        let filters = gtk::Entry::builder()
            .placeholder_text("Filters: *.rs *.toml !target/")
            .text("*.*")
            .width_chars(18)
            .tooltip_text(
                "File names to search, separated by spaces: *.rs *.toml; !*.min.js leaves \
                 files out, !\\dir a folder directly inside, !+\\dir or dir/ a folder anywhere",
            )
            .build();
        let subfolders = gtk::CheckButton::builder()
            .label("Subfolders")
            .active(true)
            .build();
        let hidden = gtk::CheckButton::builder().label("Hidden").build();
        let gitignore = gtk::CheckButton::builder()
            .label(".gitignore")
            .active(true)
            .tooltip_text("Skip what .gitignore and .ignore files ignore")
            .build();
        let find_in_files = text_button("Find in Files", "Search the folder (Enter)");
        find_in_files.add_css_class("suggested-action");
        let replace_in_files = text_button(
            "Replace in Files",
            "Replace every match in the folder's files, after a confirmation",
        );
        let files_row = row(&[
            directory.upcast_ref(),
            browse.upcast_ref(),
            filters.upcast_ref(),
            subfolders.upcast_ref(),
            hidden.upcast_ref(),
            gitignore.upcast_ref(),
            find_in_files.upcast_ref(),
            replace_in_files.upcast_ref(),
        ]);
        files_row.set_visible(false);

        let mark_style = gtk::DropDown::from_strings(&MARK_STYLES.map(|(_, label)| label));
        mark_style.set_tooltip_text(Some("The style Mark All uses"));
        let bookmark_line = gtk::CheckButton::builder()
            .label("Bookmark line")
            .tooltip_text("Also bookmark every line with a match")
            .build();
        let purge = gtk::CheckButton::builder()
            .label("Purge for each search")
            .tooltip_text("Clear the style's marks (and, with Bookmark line, the bookmarks) first")
            .build();
        let mark_all = text_button("Mark All", "Mark every match (Enter)");
        mark_all.add_css_class("suggested-action");
        let clear_marks = text_button(
            "Clear All Marks",
            "Clear the style's marks, and the bookmarks with Bookmark line",
        );
        let copy_marked = text_button("Copy Marked Text", "Copy the text the style marks");
        let mark_spacer = gtk::Box::builder().hexpand(true).build();
        let mark_row = row(&[
            mark_style.upcast_ref(),
            bookmark_line.upcast_ref(),
            purge.upcast_ref(),
            mark_spacer.upcast_ref(),
            mark_all.upcast_ref(),
            clear_marks.upcast_ref(),
            copy_marked.upcast_ref(),
        ]);
        mark_row.set_visible(false);

        let bar = gtk::Box::new(gtk::Orientation::Vertical, 0);
        bar.add_css_class("stet-findbar");
        bar.append(&find_row);
        bar.append(&replace_row);
        bar.append(&files_row);
        bar.append(&mark_row);
        // A slide of zero duration: with the None transition a hidden revealer keeps its size.
        let revealer = gtk::Revealer::builder()
            .child(&bar)
            .transition_type(gtk::RevealerTransitionType::SlideUp)
            .transition_duration(0)
            .reveal_child(false)
            .build();
        // Added, not set: setting css-classes would drop the entries' own classes.
        for field in [
            entry.upcast_ref::<gtk::Widget>(),
            replace_entry.upcast_ref(),
            directory.upcast_ref(),
        ] {
            field.add_css_class("stet-find-field");
        }
        filters.add_css_class("stet-filters");
        let keys = key_controller(&entry);
        let replace_keys = key_controller(&replace_entry);
        let directory_keys = key_controller(&directory);
        let filters_keys = key_controller(&filters);
        Self {
            revealer,
            entry,
            find_history,
            case,
            word,
            in_selection,
            mode,
            previous,
            next,
            count,
            message,
            wrap,
            backward,
            dot_newline,
            count_button,
            find_all_document,
            find_all_open,
            close,
            replace_row,
            replace_entry,
            replace_history,
            replace_button,
            replace_all_button,
            replace_open_button,
            files_row,
            replace_in_files,
            directory,
            browse,
            filters,
            subfolders,
            hidden,
            gitignore,
            find_in_files,
            mark_row,
            mark_style,
            bookmark_line,
            purge,
            mark_all,
            clear_marks,
            copy_marked,
            keys,
            replace_keys,
            directory_keys,
            filters_keys,
            stats: RefCell::new(None),
            bar_mode: Cell::new(BarMode::Find),
            compiled: RefCell::new(None),
            compiled_text: RefCell::new(String::new()),
            next_id: Cell::new(0),
            error: RefCell::new(None),
            history: RefCell::new(SearchHistory::default()),
            recalls: RefCell::new(Default::default()),
            recalled: RefCell::new(Default::default()),
            recalling: Cell::new(false),
        }
    }

    pub fn is_open(&self) -> bool {
        self.revealer.reveals_child()
    }

    /// The style the Mark row has chosen.
    pub fn mark_style(&self) -> MarkStyle {
        MARK_STYLES
            .get(self.mark_style.selected() as usize)
            .map_or(MarkStyle::Mark, |(style, _)| *style)
    }

    pub fn set_mark_style(&self, style: MarkStyle) {
        if let Some(index) = MARK_STYLES.iter().position(|(each, _)| *each == style) {
            self.mark_style.set_selected(index as u32);
        }
    }

    pub fn bar_mode(&self) -> BarMode {
        self.bar_mode.get()
    }

    pub fn search_mode(&self) -> SearchMode {
        MODES
            .get(self.mode.selected() as usize)
            .map_or(SearchMode::Normal, |(mode, _)| *mode)
    }

    pub fn set_search_mode(&self, mode: SearchMode) {
        if let Some(index) = MODES.iter().position(|(candidate, _)| *candidate == mode) {
            self.mode.set_selected(index as u32);
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

    /// The compiled query, while the find field holds a valid one.
    pub fn compiled(&self) -> Option<Rc<Compiled>> {
        self.compiled.borrow().clone()
    }

    pub fn pattern_error(&self) -> Option<PatternError> {
        self.error.borrow().clone()
    }

    pub fn history(&self) -> SearchHistory {
        self.history.borrow().clone()
    }

    pub fn set_history(&self, history: SearchHistory) {
        self.history.replace(history);
        *self.recalls.borrow_mut() = Default::default();
    }

    fn history_of(history: &SearchHistory, field: Field) -> &History {
        match field {
            Field::Find => &history.find,
            Field::Replace => &history.replace,
            Field::Directory => &history.directories,
            Field::Filters => &history.filters,
        }
    }

    /// Remembers `text` in `field`'s history.
    pub fn remember(&self, field: Field, text: &str) {
        let mut history = self.history.borrow_mut();
        let list = match field {
            Field::Find => &mut history.find,
            Field::Replace => &mut history.replace,
            Field::Directory => &mut history.directories,
            Field::Filters => &mut history.filters,
        };
        if list.add(text) {
            self.recalls.borrow_mut()[field.index()].reset();
            self.recalled.borrow_mut()[field.index()] = None;
        }
    }

    fn editable(&self, field: Field) -> gtk::Editable {
        match field {
            Field::Find => self.entry.clone().upcast(),
            Field::Replace => self.replace_entry.clone().upcast(),
            Field::Directory => self.directory.clone().upcast(),
            Field::Filters => self.filters.clone().upcast(),
        }
    }

    /// Up (`older`) and Down in a field walk its history.
    pub fn recall(&self, field: Field, older: bool) -> bool {
        let editable = self.editable(field);
        let history = self.history.borrow();
        let list = Self::history_of(&history, field);
        let mut recalls = self.recalls.borrow_mut();
        let recall = &mut recalls[field.index()];
        let text = if older {
            recall.older(list, &editable.text())
        } else {
            recall.newer(list)
        };
        drop(recalls);
        drop(history);
        match text {
            Some(text) => {
                self.recalled.borrow_mut()[field.index()] = Some(text.clone());
                // Setting the text deletes the old one first; neither change ends the walk.
                self.recalling.set(true);
                editable.set_text(&text);
                self.recalling.set(false);
                editable.set_position(-1);
                true
            }
            None => false,
        }
    }

    /// A field's text changed: unless the history walk put it there, the walk ends.
    fn field_changed(&self, field: Field) {
        if self.recalling.get() {
            return;
        }
        let text = self.editable(field).text();
        let mut recalled = self.recalled.borrow_mut();
        if recalled[field.index()].as_deref() != Some(text.as_str()) {
            recalled[field.index()] = None;
            self.recalls.borrow_mut()[field.index()].reset();
        }
    }

    /// The find field's text when it was last compiled.
    pub fn compiled_text(&self) -> String {
        self.compiled_text.borrow().clone()
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
                    underline.set_start_index(start as u32);
                    underline.set_end_index(end.max(start + 1) as u32);
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
}

impl Window {
    pub(super) fn connect_find_bar(&self) {
        let find = &self.inner.find;
        for (keys, field) in [
            (&find.keys, Field::Find),
            (&find.replace_keys, Field::Replace),
            (&find.directory_keys, Field::Directory),
            (&find.filters_keys, Field::Filters),
        ] {
            keys.connect_key_pressed(glib::clone!(
                #[weak(rename_to = inner)]
                self.inner,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, key, _, state| Window { inner }.on_bar_key(field, key, state)
            ));
        }
        find.entry.connect_search_changed(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |_| Window { inner }.on_query_changed(true)
        ));
        for field in [
            Field::Find,
            Field::Replace,
            Field::Directory,
            Field::Filters,
        ] {
            find.editable(field).connect_changed(glib::clone!(
                #[weak(rename_to = inner)]
                self.inner,
                move |_| inner.find.field_changed(field)
            ));
        }
        find.replace_entry.connect_changed(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |_| {
                let _ = Window { inner }.template();
            }
        ));
        for button in [&find.case, &find.word] {
            button.connect_toggled(glib::clone!(
                #[weak(rename_to = inner)]
                self.inner,
                move |_| Window { inner }.on_query_changed(false)
            ));
        }
        find.dot_newline.connect_toggled(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |_| Window { inner }.on_query_changed(false)
        ));
        find.mode.connect_selected_notify(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |_| {
                let window = Window { inner };
                let regex = window.inner.find.search_mode() == SearchMode::Regex;
                window.inner.find.word.set_sensitive(!regex);
                window.inner.find.dot_newline.set_sensitive(regex);
                let _ = window.template();
                window.on_query_changed(false);
            }
        ));
        find.wrap.connect_toggled(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |_| Window { inner }.sync_search_highlight()
        ));
        for (button, forward) in [(&find.previous, false), (&find.next, true)] {
            button.connect_clicked(glib::clone!(
                #[weak(rename_to = inner)]
                self.inner,
                move |_| Window { inner }.find_step(forward)
            ));
        }
        find.close.connect_clicked(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |_| Window { inner }.hide_find()
        ));
        for (button, id) in [
            (&find.count_button, stet_domain::actions::ActionId::Count),
            (
                &find.find_all_document,
                stet_domain::actions::ActionId::FindAllInDocument,
            ),
            (
                &find.find_all_open,
                stet_domain::actions::ActionId::FindAllInOpenDocuments,
            ),
            (
                &find.replace_button,
                stet_domain::actions::ActionId::ReplaceNext,
            ),
            (
                &find.replace_all_button,
                stet_domain::actions::ActionId::ReplaceAll,
            ),
            (
                &find.replace_open_button,
                stet_domain::actions::ActionId::ReplaceAllInOpenDocuments,
            ),
            (
                &find.find_in_files,
                stet_domain::actions::ActionId::FindInFiles,
            ),
            (
                &find.replace_in_files,
                stet_domain::actions::ActionId::ReplaceInFiles,
            ),
            (&find.mark_all, stet_domain::actions::ActionId::MarkAll),
            (
                &find.clear_marks,
                stet_domain::actions::ActionId::ClearMarks,
            ),
            (
                &find.copy_marked,
                stet_domain::actions::ActionId::CopyMarkedText,
            ),
        ] {
            button.connect_clicked(glib::clone!(
                #[weak(rename_to = inner)]
                self.inner,
                move |button| {
                    let window = Window { inner };
                    if let Some(popover) = button.ancestor(gtk::Popover::static_type()) {
                        popover
                            .downcast_ref::<gtk::Popover>()
                            .expect("a popover")
                            .popdown();
                    }
                    match id {
                        stet_domain::actions::ActionId::FindInFiles => {
                            window.start_find_in_files();
                        }
                        stet_domain::actions::ActionId::MarkAll
                        | stet_domain::actions::ActionId::ClearMarks
                        | stet_domain::actions::ActionId::CopyMarkedText => {
                            window.run_mark_action(id);
                        }
                        _ => window.run_search_action(id),
                    }
                }
            ));
        }
        find.browse.connect_clicked(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |_| {
                let window = Window { inner };
                let clone = window.clone();
                window.spawn(async move { clone.choose_search_folder().await });
            }
        ));
        for (history, field) in [
            (&find.find_history, Field::Find),
            (&find.replace_history, Field::Replace),
        ] {
            let Some(popover) = history.button.popover() else {
                continue;
            };
            popover.connect_show(glib::clone!(
                #[weak(rename_to = inner)]
                self.inner,
                move |_| Window { inner }.fill_history(field)
            ));
        }
    }

    fn fill_history(&self, field: Field) {
        let find = &self.inner.find;
        let entries = FindBar::history_of(&find.history.borrow(), field)
            .entries()
            .to_vec();
        let weak = Rc::downgrade(&self.inner);
        let chosen: OnChosen = Rc::new(move |text| {
            if let Some(inner) = weak.upgrade() {
                let editable = inner.find.editable(field);
                editable.set_text(&text);
                editable.grab_focus();
                editable.set_position(-1);
            }
        });
        let button = match field {
            Field::Replace => &find.replace_history,
            _ => &find.find_history,
        };
        button.fill(&entries, chosen);
    }

    fn on_bar_key(
        &self,
        field: Field,
        key: gdk::Key,
        state: gdk::ModifierType,
    ) -> glib::Propagation {
        let find = &self.inner.find;
        let shift = state.contains(gdk::ModifierType::SHIFT_MASK);
        let plain = !state.intersects(
            gdk::ModifierType::CONTROL_MASK
                | gdk::ModifierType::ALT_MASK
                | gdk::ModifierType::SUPER_MASK,
        );
        match key {
            gdk::Key::Return | gdk::Key::KP_Enter | gdk::Key::ISO_Enter if plain => {
                match field {
                    Field::Directory | Field::Filters => self.start_find_in_files(),
                    Field::Find | Field::Replace if find.bar_mode() == BarMode::Files => {
                        self.start_find_in_files();
                    }
                    Field::Find if find.bar_mode() == BarMode::Mark => self.mark_all(),
                    Field::Find => self.find_step(!shift),
                    Field::Replace => self.replace_next(!shift),
                }
                glib::Propagation::Stop
            }
            gdk::Key::Escape => {
                self.hide_find();
                glib::Propagation::Stop
            }
            gdk::Key::Up | gdk::Key::Down if plain && !shift => {
                find.recall(field, key == gdk::Key::Up);
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    }

    /// The find field or an option that changes the query changed: compile it again, show it
    /// in the visible tab, and with `incremental` move to the first match from the caret.
    fn on_query_changed(&self, incremental: bool) {
        let find = &self.inner.find;
        find.set_message("", Tone::Info);
        let compiled = find.compile();
        self.sync_search_highlight();
        // The field's change arrives after a short delay; once the bar is closed, it must not
        // move the selection any more.
        if incremental && compiled.is_some() && find.is_open() {
            self.find_from_selection_start();
        }
        self.update_match_count();
    }

    /// Opens the find bar (Ctrl+F), prefilled with a single-line selection.
    pub fn show_find(&self) {
        self.show_find_mode(BarMode::Find);
    }

    /// Opens the bar with the Find, Replace, Find in Files or Mark rows.
    pub fn show_find_mode(&self, mode: BarMode) {
        let find = &self.inner.find;
        let opening = !find.is_open();
        let entering = opening || find.bar_mode() != mode;
        if let Some(page) = self.current_page() {
            let buffer = page.buffer();
            match buffer.selection_bounds() {
                Some((start, end)) => {
                    let selected = buffer.text(&start, &end, false);
                    if selected.contains('\n') {
                        self.remember_search_scope(&page);
                    } else {
                        super::replace::forget_search_scope(&page);
                        find.entry.set_text(&selected);
                    }
                }
                None if opening => super::replace::forget_search_scope(&page),
                None => {}
            }
        }
        if opening {
            self.note_jump();
        }
        find.bar_mode.set(mode);
        // Find in Files has the replace field too, for Replace in Files; the buttons that
        // replace in the document belong to Replace (M8).
        find.replace_row
            .set_visible(matches!(mode, BarMode::Replace | BarMode::Files));
        for button in [
            &find.replace_button,
            &find.replace_all_button,
            &find.replace_open_button,
        ] {
            button.set_visible(mode == BarMode::Replace);
        }
        find.files_row.set_visible(mode == BarMode::Files);
        find.mark_row.set_visible(mode == BarMode::Mark);
        if mode == BarMode::Files && (entering || find.directory.text().trim().is_empty()) {
            self.default_search_folder();
        }
        find.revealer.set_reveal_child(true);
        find.entry.grab_focus();
        find.entry.select_region(0, -1);
        if find.compiled().is_none() && find.pattern_error().is_none() {
            find.compile();
        }
        self.sync_search_highlight();
        self.update_match_count();
    }

    /// Closes the bar. The current match becomes the selection, so typing or copying acts on it.
    pub fn hide_find(&self) {
        self.inner.find.revealer.set_reveal_child(false);
        if let Some(page) = self.current_page() {
            if let Some((start, end)) = page.current_match() {
                page.buffer().select_range(&end, &start);
            }
            super::replace::forget_search_scope(&page);
            page.view().grab_focus();
        }
        self.sync_search_highlight();
    }

    /// Hooks a newly created find context to the match count.
    fn context_for(&self, page: &EditorPage) -> sourceview5::SearchContext {
        let (context, created) = page.search().context(&page.buffer());
        if created {
            context.connect_occurrences_count_notify(glib::clone!(
                #[weak(rename_to = inner)]
                self.inner,
                move |_| Window { inner }.update_match_count()
            ));
            context.connect_regex_error_notify(glib::clone!(
                #[weak(rename_to = inner)]
                self.inner,
                move |context| {
                    if let Some(error) = context.regex_error() {
                        Window { inner }
                            .inner
                            .find
                            .set_message(&error.to_string(), Tone::Error);
                    }
                }
            ));
        }
        context
    }

    /// Whether the bar shows every match in `page`: while it is open, except in the Mark row,
    /// where the marks show what Mark All found, and never in large-file mode.
    pub(super) fn shows_matches(&self, page: &EditorPage) -> bool {
        let find = &self.inner.find;
        find.is_open() && find.bar_mode() != BarMode::Mark && !page.large_file_mode()
    }

    /// Puts the query into the visible tab while the bar is open: GtkSourceView's highlight,
    /// or our matcher's matches. Other tabs show nothing and keep their settings, so they
    /// never rescan in the background.
    pub(super) fn sync_search_highlight(&self) {
        let find = &self.inner.find;
        let open = find.is_open();
        let current = self.current_page();
        let compiled = find.compiled();
        let wrap = find.wrap.is_active();
        // Per document: one in both views (M7) is searched once, in the current view.
        for page in self.documents() {
            let visible = open && Some(&page) == current.as_ref();
            let state = page.search();
            let buffer = page.buffer();
            if !visible {
                if let Some(context) = state.existing_context() {
                    context.set_highlight(false);
                }
                state.clear_own(&buffer);
                page.clear_current_match();
                continue;
            }
            match &compiled {
                Some(compiled) if !compiled.own() => {
                    state.clear_own(&buffer);
                    let context = self.context_for(&page);
                    state.apply(Some(compiled), wrap);
                    context.set_highlight(self.shows_matches(&page));
                }
                Some(compiled) => {
                    state.apply(Some(compiled), wrap);
                    if let Some(context) = state.existing_context() {
                        context.set_highlight(false);
                    }
                    self.refresh_own_matches(&page, compiled);
                }
                None => {
                    state.apply(None, wrap);
                    state.clear_own(&buffer);
                }
            }
        }
    }

    /// Searches the visible tab with our own matcher, on a snapshot on a worker, unless its
    /// matches for this query and revision are known.
    fn refresh_own_matches(&self, page: &EditorPage, compiled: &Rc<Compiled>) {
        let state = page.search();
        let revision = page.revision();
        if state.own_matches(compiled.id, revision).is_some() {
            state.tag_own(page.view(), revision, self.shows_matches(page));
            return;
        }
        if state.own_pending.get() == Some((compiled.id, revision)) {
            return;
        }
        if let Some(cancel) = state.own_cancel.take() {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        let generation = state.own_generation.get() + 1;
        state.own_generation.set(generation);
        state.own_pending.set(Some((compiled.id, revision)));
        let cancel = Arc::new(AtomicBool::new(false));
        state.own_cancel.replace(Some(Arc::clone(&cancel)));
        let text = page.text();
        let matcher = Arc::clone(&compiled.matcher);
        let id = compiled.id;
        let window = self.clone();
        let page = page.clone();
        self.spawn(async move {
            let found =
                worker::run(move || matcher.find_all(&text, None, OWN_MATCH_LIMIT, &cancel)).await;
            let state = page.search();
            if state.own_generation.get() != generation {
                return;
            }
            state.own_pending.set(None);
            state.own_cancel.replace(None);
            if page.revision() != revision {
                return;
            }
            match found {
                Some(Ok(found)) => {
                    state.own.replace(Some(OwnMatches {
                        query: id,
                        revision,
                        matches: Arc::new(found.matches),
                        truncated: found.truncated,
                    }));
                    state.tag_own(page.view(), revision, window.shows_matches(&page));
                }
                Some(Err(error)) => window
                    .inner
                    .find
                    .set_message(&error.to_string(), Tone::Error),
                None => {}
            }
            window.update_match_count();
        });
    }

    /// A new tab's search signals: edits refresh our matcher's matches, caret moves end a
    /// current match, selection changes update smart highlighting, and scrolling moves our
    /// matcher's highlight along.
    pub(super) fn connect_page_search(&self, page: &EditorPage) {
        let buffer = page.buffer();
        buffer.connect_changed(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            #[weak]
            page,
            move |_| Window { inner }.on_buffer_changed(&page)
        ));
        buffer.connect_mark_set(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            #[weak]
            page,
            move |_, _, mark| {
                let window = Window { inner };
                match mark.name().as_deref() {
                    Some("insert") => {
                        window.on_caret_moved(&page);
                        window.on_selection_changed(&page);
                    }
                    Some("selection_bound") => window.on_selection_changed(&page),
                    _ => {}
                }
            }
        ));
        if let Some(adjustment) = page.view().vadjustment() {
            adjustment.connect_value_changed(glib::clone!(
                #[weak(rename_to = inner)]
                self.inner,
                #[weak]
                page,
                move |_| Window { inner }.on_editor_scrolled(&page)
            ));
        }
    }

    /// The buffer of `page` changed: our matcher searches again shortly after.
    pub(super) fn on_buffer_changed(&self, page: &EditorPage) {
        let find = &self.inner.find;
        if !find.is_open() || self.current_page().as_ref() != Some(page) {
            return;
        }
        let Some(compiled) = find.compiled().filter(|compiled| compiled.own()) else {
            return;
        };
        let state = page.search();
        if let Some(source) = state.own_timeout.take() {
            source.remove();
        }
        // A large document's snapshot costs the GTK thread up to about 0.1 s; wait longer.
        let delay = if page.large_file_mode() {
            OWN_REFRESH_DELAY_LARGE
        } else {
            OWN_REFRESH_DELAY
        };
        let window = self.clone();
        let page_ref = page.clone();
        let source = glib::timeout_add_local_once(delay, move || {
            page_ref.search().own_timeout.replace(None);
            let current = window.inner.find.compiled();
            if current
                .as_ref()
                .is_some_and(|current| current.id == compiled.id)
            {
                window.refresh_own_matches(&page_ref, &compiled);
            }
        });
        state.own_timeout.replace(Some(source));
    }

    /// The visible tab scrolled: our matcher's highlight follows the visible lines.
    pub(super) fn on_editor_scrolled(&self, page: &EditorPage) {
        if self.inner.find.is_open()
            && self.current_page().as_ref() == Some(page)
            && page.search().own.borrow().is_some()
        {
            page.search()
                .tag_own(page.view(), page.revision(), self.shows_matches(page));
        }
    }

    /// The caret moved: a current match it left no longer anchors the next search.
    pub(super) fn on_caret_moved(&self, page: &EditorPage) {
        let Some((start, end)) = page.current_match() else {
            return;
        };
        let buffer = page.buffer();
        let caret = buffer.iter_at_mark(&buffer.get_insert());
        if caret != start && caret != end {
            page.clear_current_match();
            self.update_match_count();
        }
    }

    /// The current match while the bar is open, else the selection, else the cursor.
    pub(super) fn search_anchor(&self, page: &EditorPage) -> (gtk::TextIter, gtk::TextIter) {
        let buffer = page.buffer();
        self.inner
            .find
            .is_open()
            .then(|| page.current_match())
            .flatten()
            .or_else(|| buffer.selection_bounds())
            .unwrap_or_else(|| {
                let cursor = buffer.iter_at_mark(&buffer.get_insert());
                (cursor, cursor)
            })
    }

    /// Find next (`forward`) or previous; Backward direction swaps them.
    pub fn find_step(&self, forward: bool) {
        let find = &self.inner.find;
        if find.compiled().is_none() {
            find.compile();
        }
        let Some(compiled) = find.compiled() else {
            self.show_find();
            return;
        };
        let Some(page) = self.current_page() else {
            return;
        };
        let forward = forward != find.backward.is_active();
        find.remember(Field::Find, &compiled.query.pattern);
        self.note_jump();
        let (start, end) = self.search_anchor(&page);
        let from = if forward { end } else { start };
        self.search_from(page, from.offset(), forward);
    }

    /// Incremental search while typing: the first match at or after the current one.
    fn find_from_selection_start(&self) {
        let Some(page) = self.current_page() else {
            return;
        };
        let (start, _) = self.search_anchor(&page);
        self.search_from(page, start.offset(), true);
    }

    /// Searches from character `from` in `page`, with GtkSourceView or our matcher.
    pub(super) fn search_from(&self, page: EditorPage, from: i32, forward: bool) {
        let find = &self.inner.find;
        let Some(compiled) = find.compiled() else {
            return;
        };
        let wrap = find.wrap.is_active();
        let state = page.search();
        state.apply(Some(&compiled), wrap);
        let window = self.clone();
        if compiled.own() {
            let text = page.text();
            let revision = page.revision();
            let matcher = Arc::clone(&compiled.matcher);
            self.spawn(async move {
                let from = from.max(0) as usize;
                let found =
                    worker::run(move || matcher.find_next(&text, from, !forward, wrap)).await;
                if page.revision() != revision {
                    return;
                }
                match found {
                    Some(Ok(Some(next))) => window.show_match(
                        &page,
                        next.found.start as i32,
                        next.found.end as i32,
                        forward,
                        next.wrapped,
                    ),
                    Some(Ok(None)) => window.not_found(forward, wrap),
                    Some(Err(error)) => window
                        .inner
                        .find
                        .set_message(&error.to_string(), Tone::Error),
                    None => {}
                }
                window.update_match_count();
            });
            return;
        }
        let context = self.context_for(&page);
        context.set_highlight(self.shows_matches(&page));
        let from = page.buffer().iter_at_offset(from);
        self.spawn(async move {
            match crate::search::find(&context, &from, forward).await {
                Some((start, end, wrapped)) => {
                    window.show_match(&page, start.offset(), end.offset(), forward, wrapped);
                }
                None => window.not_found(forward, wrap),
            }
            window.update_match_count();
        });
    }

    /// While the bar has focus the match is highlighted with the theme's current-match style
    /// (a selection would be drawn in the faint unfocused colour); otherwise it is selected.
    /// The caret goes to the far end of the match in the search direction.
    pub(super) fn show_match(
        &self,
        page: &EditorPage,
        start: i32,
        end: i32,
        forward: bool,
        wrapped: bool,
    ) {
        let buffer = page.buffer();
        let start = buffer.iter_at_offset(start);
        let end = buffer.iter_at_offset(end);
        let caret = if forward { &end } else { &start };
        if self.inner.find.is_open() {
            page.set_current_match(&start, &end);
            buffer.place_cursor(caret);
        } else if forward {
            buffer.select_range(&end, &start);
        } else {
            buffer.select_range(&start, &end);
        }
        page.view()
            .scroll_to_mark(&buffer.get_insert(), 0.1, false, 0.0, 0.0);
        let message = match (wrapped, forward) {
            (false, _) => "",
            (true, true) => "Passed the end of the document; continued from the start",
            (true, false) => "Passed the start of the document; continued from the end",
        };
        self.inner.find.set_message(message, Tone::Info);
    }

    fn not_found(&self, forward: bool, wrap: bool) {
        let message = match (wrap, forward) {
            (true, _) => "No match",
            (false, true) => "No match from the caret to the end",
            (false, false) => "No match from the start to the caret",
        };
        self.inner.find.set_message(message, Tone::Info);
    }

    pub(super) fn update_match_count(&self) {
        let find = &self.inner.find;
        let pattern = find.entry.text();
        let label = match (self.current_page(), find.compiled()) {
            _ if pattern.is_empty() => String::new(),
            (_, None) => "Invalid pattern".to_owned(),
            (None, _) => String::new(),
            (Some(page), Some(compiled)) if compiled.own() => {
                match page.search().own_matches(compiled.id, page.revision()) {
                    None => "Searching…".to_owned(),
                    Some((matches, truncated)) => {
                        let count = matches.len();
                        let (start, end) = self.search_anchor(&page);
                        let position = page
                            .search()
                            .own_position(start.offset() as usize, end.offset() as usize);
                        let more = if truncated { "+" } else { "" };
                        match (count, position) {
                            (0, _) => "No matches".to_owned(),
                            (count, Some(position)) => format!("{position} of {count}{more}"),
                            (1, None) => "1 match".to_owned(),
                            (count, None) => format!("{count}{more} matches"),
                        }
                    }
                }
            }
            (Some(page), Some(_)) => {
                let context = self.context_for(&page);
                let count = context.occurrences_count();
                let (start, end) = self.search_anchor(&page);
                let position = context.occurrence_position(&start, &end);
                match count {
                    -1 => "Searching…".to_owned(),
                    0 => "No matches".to_owned(),
                    count if position > 0 => format!("{position} of {count}"),
                    1 => "1 match".to_owned(),
                    count => format!("{count} matches"),
                }
            }
        };
        find.count.set_label(&label);
        if let Some(page) = self.current_page() {
            page.keep_current_match_on_top();
        }
    }

    /// The histories, for the session (M2) to save: find and replace map to
    /// `session.json`'s `find_history` and `replace_history`.
    pub fn search_history(&self) -> SearchHistory {
        self.inner.find.history()
    }

    /// Restores the histories, for example from the session.
    pub fn set_search_history(&self, history: SearchHistory) {
        self.inner.find.set_history(history);
    }

    /// The replace field read in the current mode, or the error that it holds.
    /// Also marks the field: the error style for a template no text can hold, the warning
    /// style with an explanation for one that is easy to get wrong.
    pub(super) fn template(&self) -> Result<stet_domain::search::Template, String> {
        let entry = &self.inner.find.replace_entry;
        let text = entry.text();
        let parsed = stet_domain::search::Template::for_mode(self.inner.find.search_mode(), &text);
        let (tooltip, class) = match &parsed {
            Ok(template) if template.warnings().is_empty() => (None, None),
            Ok(template) => {
                let warnings: Vec<String> = template
                    .warnings()
                    .iter()
                    .map(|warning| template_warning(*warning))
                    .collect();
                (Some(warnings.join("\n")), Some("warning"))
            }
            Err(error) => (
                Some(TranslateError::from(error.clone()).to_string()),
                Some("error"),
            ),
        };
        for style in ["error", "warning"] {
            if Some(style) == class {
                entry.add_css_class(style);
            } else {
                entry.remove_css_class(style);
            }
        }
        entry.set_tooltip_text(tooltip.as_deref());
        parsed.map_err(|error| TranslateError::from(error).to_string())
    }
}
