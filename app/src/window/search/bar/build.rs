//! The bar's widget tree, a row at a time from top to bottom: find (with the options
//! popover), replace, files and mark, then the revealer that keeps them.

use super::history::HistoryButton;
use super::{BarMode, FindBar, MARK_STYLES, MODES};
use crate::runtime::Flag;
use gtk4 as gtk;
use gtk4::pango;
use gtk4::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use stet_domain::search::SearchHistory;

/// The find row: the field, its history, the toggles, the mode, the arrows, the count and the
/// message, the options popover and the close button.
struct FindRow {
    row: gtk::Box,
    entry: gtk::SearchEntry,
    history: HistoryButton,
    case: gtk::ToggleButton,
    word: gtk::ToggleButton,
    in_selection: gtk::ToggleButton,
    mode: gtk::DropDown,
    previous: gtk::Button,
    next: gtk::Button,
    count: gtk::Label,
    message: gtk::Label,
    options: OptionsPopover,
    close: gtk::Button,
}

/// The find row's popover: the options that are seldom changed, Count and Find All.
struct OptionsPopover {
    button: gtk::MenuButton,
    wrap: gtk::CheckButton,
    backward: gtk::CheckButton,
    dot_newline: gtk::CheckButton,
    count: gtk::Button,
    find_all_document: gtk::Button,
    find_all_open: gtk::Button,
}

struct ReplaceRow {
    row: gtk::Box,
    entry: gtk::Entry,
    history: HistoryButton,
    replace: gtk::Button,
    replace_all: gtk::Button,
    replace_open: gtk::Button,
}

struct FilesRow {
    row: gtk::Box,
    directory: gtk::Entry,
    browse: gtk::Button,
    filters: gtk::Entry,
    subfolders: gtk::CheckButton,
    hidden: gtk::CheckButton,
    gitignore: gtk::CheckButton,
    find_in_files: gtk::Button,
    replace_in_files: gtk::Button,
}

struct MarkRow {
    row: gtk::Box,
    style: gtk::DropDown,
    bookmark_line: gtk::CheckButton,
    purge: gtk::CheckButton,
    mark_all: gtk::Button,
    clear_marks: gtk::Button,
    copy_marked: gtk::Button,
}

impl FindBar {
    pub fn new() -> Rc<Self> {
        let find = find_row();
        let replace = replace_row();
        let files = files_row();
        let mark = mark_row();

        let bar = gtk::Box::new(gtk::Orientation::Vertical, 0);
        bar.add_css_class("stet-findbar");
        bar.append(&find.row);
        bar.append(&replace.row);
        bar.append(&files.row);
        bar.append(&mark.row);
        // A slide of zero duration: with the None transition a hidden revealer keeps its size.
        let revealer = gtk::Revealer::builder()
            .child(&bar)
            .transition_type(gtk::RevealerTransitionType::SlideUp)
            .transition_duration(0)
            .reveal_child(false)
            .build();
        // Added, not set: setting css-classes would drop the entries' own classes.
        for field in [
            find.entry.upcast_ref::<gtk::Widget>(),
            replace.entry.upcast_ref(),
            files.directory.upcast_ref(),
        ] {
            field.add_css_class("stet-find-field");
        }
        files.filters.add_css_class("stet-filters");
        let keys = [
            key_controller(&find.entry),
            key_controller(&replace.entry),
            key_controller(&files.directory),
            key_controller(&files.filters),
        ];
        let bar = Rc::new(Self {
            revealer,
            entry: find.entry,
            find_history: find.history,
            case: find.case,
            word: find.word,
            in_selection: find.in_selection,
            mode: find.mode,
            previous: find.previous,
            next: find.next,
            count: find.count,
            message: find.message,
            wrap: find.options.wrap,
            backward: find.options.backward,
            dot_newline: find.options.dot_newline,
            count_button: find.options.count,
            find_all_document: find.options.find_all_document,
            find_all_open: find.options.find_all_open,
            close: find.close,
            replace_row: replace.row,
            replace_entry: replace.entry,
            replace_history: replace.history,
            replace_button: replace.replace,
            replace_all_button: replace.replace_all,
            replace_open_button: replace.replace_open,
            files_row: files.row,
            replace_in_files: files.replace_in_files,
            directory: files.directory,
            browse: files.browse,
            filters: files.filters,
            subfolders: files.subfolders,
            hidden: files.hidden,
            gitignore: files.gitignore,
            find_in_files: files.find_in_files,
            mark_row: mark.row,
            mark_style: mark.style,
            bookmark_line: mark.bookmark_line,
            purge: mark.purge,
            mark_all: mark.mark_all,
            clear_marks: mark.clear_marks,
            copy_marked: mark.copy_marked,
            keys,
            bar_mode: Cell::new(BarMode::Find),
            compiled: RefCell::new(None),
            compiled_text: RefCell::new(String::new()),
            next_id: Cell::new(0),
            error: RefCell::new(None),
            history: RefCell::new(SearchHistory::default()),
            recalls: RefCell::new(Default::default()),
            recalled: RefCell::new(Default::default()),
            recalling: Flag::default(),
        });
        bar.connect_own();
        bar
    }
}

fn find_row() -> FindRow {
    let entry = gtk::SearchEntry::builder()
        .placeholder_text("Find")
        .search_delay(80)
        .hexpand(true)
        .build();
    // A placeholder is no accessible name: screen readers announced these fields unnamed.
    entry.update_property(&[gtk::accessible::Property::Label("Find")]);
    let history = HistoryButton::new("Find history (↑ and ↓ in the field)");
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
    let options = options_popover();
    let close = icon_button("window-close-symbolic", "Close (Escape)");

    let row = row(&[
        entry.upcast_ref(),
        history.button.upcast_ref(),
        case.upcast_ref(),
        word.upcast_ref(),
        in_selection.upcast_ref(),
        mode.upcast_ref(),
        previous.upcast_ref(),
        next.upcast_ref(),
        count.upcast_ref(),
        message.upcast_ref(),
        options.button.upcast_ref(),
        close.upcast_ref(),
    ]);
    FindRow {
        row,
        entry,
        history,
        case,
        word,
        in_selection,
        mode,
        previous,
        next,
        count,
        message,
        options,
        close,
    }
}

fn options_popover() -> OptionsPopover {
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
    let count = menu_button("Count");
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
    for widget in [&count, &find_all_document, &find_all_open] {
        options.append(widget);
    }
    let button = gtk::MenuButton::builder()
        .icon_name("view-more-symbolic")
        .tooltip_text("More options, Count and Find All")
        .popover(&gtk::Popover::builder().child(&options).build())
        .build();
    button.add_css_class("flat");
    OptionsPopover {
        button,
        wrap,
        backward,
        dot_newline,
        count,
        find_all_document,
        find_all_open,
    }
}

fn replace_row() -> ReplaceRow {
    let entry = gtk::Entry::builder()
        .placeholder_text("Replace with")
        .hexpand(true)
        .build();
    entry.update_property(&[gtk::accessible::Property::Label("Replace with")]);
    let history = HistoryButton::new("Replace history (↑ and ↓ in the field)");
    let replace = text_button(
        "Replace",
        "Replace the match and find the next one (Enter in this field)",
    );
    let replace_all = text_button("Replace All", "Replace every match");
    let replace_open = text_button(
        "Replace All in Open Documents",
        "Replace every match in every open document",
    );
    let row = row(&[
        entry.upcast_ref(),
        history.button.upcast_ref(),
        replace.upcast_ref(),
        replace_all.upcast_ref(),
        replace_open.upcast_ref(),
    ]);
    row.set_visible(false);
    ReplaceRow {
        row,
        entry,
        history,
        replace,
        replace_all,
        replace_open,
    }
}

fn files_row() -> FilesRow {
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
    let row = row(&[
        directory.upcast_ref(),
        browse.upcast_ref(),
        filters.upcast_ref(),
        subfolders.upcast_ref(),
        hidden.upcast_ref(),
        gitignore.upcast_ref(),
        find_in_files.upcast_ref(),
        replace_in_files.upcast_ref(),
    ]);
    row.set_visible(false);
    FilesRow {
        row,
        directory,
        browse,
        filters,
        subfolders,
        hidden,
        gitignore,
        find_in_files,
        replace_in_files,
    }
}

fn mark_row() -> MarkRow {
    let style = gtk::DropDown::from_strings(&MARK_STYLES.map(|(_, label)| label));
    style.set_tooltip_text(Some("The style Mark All uses"));
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
    let spacer = gtk::Box::builder().hexpand(true).build();
    let row = row(&[
        style.upcast_ref(),
        bookmark_line.upcast_ref(),
        purge.upcast_ref(),
        spacer.upcast_ref(),
        mark_all.upcast_ref(),
        clear_marks.upcast_ref(),
        copy_marked.upcast_ref(),
    ]);
    row.set_visible(false);
    MarkRow {
        row,
        style,
        bookmark_line,
        purge,
        mark_all,
        clear_marks,
        copy_marked,
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

/// A flat button in the options popover, its label at the start like a menu item's.
fn menu_button(label: &str) -> gtk::Button {
    let button = gtk::Button::builder().label(label).build();
    button.add_css_class("flat");
    if let Some(child) = button.child().and_downcast::<gtk::Label>() {
        child.set_xalign(0.0);
    }
    button
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
