//! The language picker: a filterable list of GtkSourceView's languages, opened from the status
//! bar or with "Set Language…".

use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use stet_domain::palette;

/// Called with the chosen language id; `None` is Plain Text.
type OnChosen = Box<dyn Fn(Option<String>)>;

struct Entry {
    id: Option<String>,
    search: String,
    row: gtk::ListBoxRow,
}

pub struct LanguagePicker {
    pub popover: gtk::Popover,
    entry: gtk::SearchEntry,
    list: gtk::ListBox,
    scrolled: gtk::ScrolledWindow,
    entries: Vec<Entry>,
    on_chosen: RefCell<Option<OnChosen>>,
}

impl LanguagePicker {
    pub fn new() -> Rc<Self> {
        let manager = sourceview5::LanguageManager::default();
        let mut languages: Vec<sourceview5::Language> = manager
            .language_ids()
            .iter()
            .filter_map(|id| manager.language(id))
            .filter(|language| !language.is_hidden())
            .collect();
        languages.sort_by_key(|language| language.name().to_lowercase());

        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Browse)
            .build();
        let mut entries = vec![Entry {
            id: None,
            search: "Plain Text text".to_owned(),
            row: row("Plain Text", ""),
        }];
        entries.extend(languages.iter().map(|language| {
            let id = language.id().to_string();
            let section = language.section().to_string();
            Entry {
                search: format!("{} {id} {section}", language.name()),
                row: row(&language.name(), &section),
                id: Some(id),
            }
        }));
        for entry in &entries {
            list.append(&entry.row);
        }

        let entry = gtk::SearchEntry::builder()
            .placeholder_text("Filter languages")
            .search_delay(0)
            .build();
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_height(320)
            .max_content_height(320)
            .build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&entry);
        content.append(&scrolled);
        let popover = gtk::Popover::builder()
            .child(&content)
            .has_arrow(false)
            .position(gtk::PositionType::Top)
            .build();
        popover.add_css_class("stet-language");
        popover.add_css_class("stet-mono");

        let picker = Rc::new(Self {
            popover,
            entry,
            list,
            scrolled,
            entries,
            on_chosen: RefCell::new(None),
        });
        picker.connect_signals();
        picker
    }

    pub fn connect_chosen(&self, on_chosen: impl Fn(Option<String>) + 'static) {
        self.on_chosen.replace(Some(Box::new(on_chosen)));
    }

    fn connect_signals(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.entry.connect_search_changed(move |_| {
            if let Some(picker) = weak.upgrade() {
                picker.filter();
            }
        });
        let weak = Rc::downgrade(self);
        self.entry.connect_activate(move |_| {
            if let Some(picker) = weak.upgrade()
                && let Some(row) = picker.list.selected_row()
            {
                picker.choose(&row);
            }
        });
        let weak = Rc::downgrade(self);
        self.list.connect_row_activated(move |_, row| {
            if let Some(picker) = weak.upgrade() {
                picker.choose(row);
            }
        });
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(picker) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            match key {
                gtk::gdk::Key::Down => picker.move_selection(1),
                gtk::gdk::Key::Up => picker.move_selection(-1),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        self.entry.add_controller(keys);
        let weak = Rc::downgrade(self);
        self.popover.connect_show(move |_| {
            if let Some(picker) = weak.upgrade() {
                picker.entry.set_text("");
                picker.filter();
                picker.entry.grab_focus();
            }
        });
    }

    /// Opens the picker with `current` selected.
    pub fn select(&self, current: Option<&str>) {
        if let Some(entry) = self
            .entries
            .iter()
            .find(|entry| entry.id.as_deref() == current)
        {
            self.list.select_row(Some(&entry.row));
        }
    }

    fn filter(&self) {
        let query = self.entry.text();
        let mut first = None;
        for entry in &self.entries {
            let visible = palette::score(&query, &entry.search).is_some();
            entry.row.set_visible(visible);
            if visible && first.is_none() {
                first = Some(entry.row.clone());
            }
        }
        if !query.is_empty() {
            self.list.select_row(first.as_ref());
        }
    }

    fn move_selection(&self, step: i32) {
        let visible: Vec<&gtk::ListBoxRow> = self
            .entries
            .iter()
            .map(|entry| &entry.row)
            .filter(|row| row.is_visible())
            .collect();
        if visible.is_empty() {
            return;
        }
        let current = self
            .list
            .selected_row()
            .and_then(|row| visible.iter().position(|visible| **visible == row));
        let next = match current {
            None => 0,
            Some(index) => (index as i32 + step).clamp(0, visible.len() as i32 - 1) as usize,
        };
        self.list.select_row(Some(visible[next]));
        scroll_to(&self.scrolled, &self.list, visible[next]);
    }

    fn choose(&self, row: &gtk::ListBoxRow) {
        let Some(entry) = self.entries.iter().find(|entry| entry.row == *row) else {
            return;
        };
        let id = entry.id.clone();
        self.popover.popdown();
        if let Some(on_chosen) = self.on_chosen.borrow().as_ref() {
            on_chosen(id);
        }
    }

    /// Chooses the first language matching `query`, as typing it and pressing Enter would.
    pub fn choose_matching(&self, query: &str) -> bool {
        let best = palette::rank(
            query,
            self.entries.iter().map(|entry| entry.search.as_str()),
        );
        match best.first() {
            Some(&index) => {
                let row = self.entries[index].row.clone();
                self.choose(&row);
                true
            }
            None => false,
        }
    }
}

/// Scrolls `row` into view without moving the keyboard focus out of the entry.
pub fn scroll_to(scrolled: &gtk::ScrolledWindow, list: &gtk::ListBox, row: &gtk::ListBoxRow) {
    let Some(top) = row.compute_point(list, &gtk::graphene::Point::new(0.0, 0.0)) else {
        return;
    };
    let top = f64::from(top.y());
    scrolled
        .vadjustment()
        .clamp_page(top, top + f64::from(row.height()));
}

fn row(name: &str, section: &str) -> gtk::ListBoxRow {
    let name = gtk::Label::builder()
        .label(name)
        .xalign(0.0)
        .hexpand(true)
        .build();
    let section = gtk::Label::builder()
        .label(section)
        .xalign(1.0)
        .css_classes(["dim-label"])
        .build();
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    content.append(&name);
    content.append(&section);
    gtk::ListBoxRow::builder().child(&content).build()
}
