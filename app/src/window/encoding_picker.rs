//! The status bar's encoding picker: every encoding in `ENCODINGS`, grouped and filterable,
//! to reinterpret the file as (read its bytes again) or to convert to (write the next save
//! in). A popover of its own rather than a menu: a menu's submenus are pages named by their
//! labels, so the same groups under Reinterpret and Convert would collide.

use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use stet_domain::palette;
use stet_infrastructure::encoding::{ENCODINGS, EncodingEntry};

/// What choosing an encoding does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerMode {
    Reinterpret,
    Convert,
}

type OnChosen = Box<dyn Fn(PickerMode, &'static EncodingEntry)>;

struct Row {
    row: gtk::ListBoxRow,
    entry: &'static EncodingEntry,
    search: String,
}

pub struct EncodingPicker {
    pub popover: gtk::Popover,
    reinterpret: gtk::ToggleButton,
    convert: gtk::ToggleButton,
    entry: gtk::SearchEntry,
    list: gtk::ListBox,
    scrolled: gtk::ScrolledWindow,
    rows: Vec<Row>,
    current: Cell<Option<&'static EncodingEntry>>,
    on_chosen: RefCell<Option<OnChosen>>,
}

impl EncodingPicker {
    pub fn new() -> Rc<Self> {
        let reinterpret = gtk::ToggleButton::builder()
            .label("Reinterpret As")
            .tooltip_text("Read the file's bytes again in another encoding")
            .hexpand(true)
            .build();
        let convert = gtk::ToggleButton::builder()
            .label("Convert To")
            .tooltip_text("Write the next save in another encoding")
            .group(&reinterpret)
            .hexpand(true)
            .build();
        let modes = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        modes.add_css_class("linked");
        modes.append(&reinterpret);
        modes.append(&convert);
        let entry = gtk::SearchEntry::builder()
            .placeholder_text("Filter encodings")
            .search_delay(0)
            .build();
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Browse)
            .build();
        let rows: Vec<Row> = ENCODINGS
            .iter()
            .map(|entry| {
                let label = gtk::Label::builder().label(entry.label).xalign(0.0).build();
                let row = gtk::ListBoxRow::builder().child(&label).build();
                list.append(&row);
                Row {
                    row,
                    entry,
                    search: format!(
                        "{} {} {}",
                        entry.label,
                        entry.encoding.name(),
                        entry.group.label()
                    ),
                }
            })
            .collect();
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_height(320)
            .max_content_height(320)
            .build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
        content.append(&modes);
        content.append(&entry);
        content.append(&scrolled);
        let popover = gtk::Popover::builder()
            .child(&content)
            .has_arrow(false)
            .position(gtk::PositionType::Top)
            .build();
        popover.add_css_class("stet-encodings");
        popover.add_css_class("stet-mono");

        let picker = Rc::new(Self {
            popover,
            reinterpret,
            convert,
            entry,
            list,
            scrolled,
            rows,
            current: Cell::new(None),
            on_chosen: RefCell::new(None),
        });
        picker.connect_signals();
        picker
    }

    pub fn connect_chosen(&self, on_chosen: impl Fn(PickerMode, &'static EncodingEntry) + 'static) {
        self.on_chosen.replace(Some(Box::new(on_chosen)));
    }

    fn connect_signals(self: &Rc<Self>) {
        let groups: Vec<(gtk::ListBoxRow, &'static str)> = self
            .rows
            .iter()
            .map(|row| (row.row.clone(), row.entry.group.label()))
            .collect();
        self.list.set_header_func(move |row, before| {
            let group = |row: &gtk::ListBoxRow| {
                groups
                    .iter()
                    .find(|(candidate, _)| candidate == row)
                    .map(|(_, group)| *group)
            };
            let label = group(row);
            if label.is_some() && before.and_then(group) != label {
                let header = gtk::Label::builder()
                    .label(label.unwrap_or_default())
                    .xalign(0.0)
                    .css_classes(["dim-label", "caption-heading"])
                    .build();
                row.set_header(Some(&header));
            } else {
                row.set_header(None::<&gtk::Widget>);
            }
        });
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
        let weak = Rc::downgrade(self);
        self.popover.connect_show(move |_| {
            if let Some(picker) = weak.upgrade() {
                picker.entry.set_text("");
                picker.filter();
                picker.select_current();
                picker.entry.grab_focus();
            }
        });
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(picker) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            match key {
                gtk::gdk::Key::Down => picker.step(1),
                gtk::gdk::Key::Up => picker.step(-1),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        self.entry.add_controller(keys);
    }

    /// Prepares the picker for a document in `current`; untitled documents can only convert.
    /// While the picker is open, the mode the user chose stays: the status bar is refreshed
    /// by loads and reloads too.
    pub fn prepare(&self, current: Option<&'static EncodingEntry>, has_file: bool) {
        self.current.set(current);
        self.reinterpret.set_sensitive(has_file);
        if !has_file {
            self.convert.set_active(true);
        } else if !self.popover.is_visible() {
            self.reinterpret.set_active(true);
        }
    }

    pub fn set_mode(&self, mode: PickerMode) {
        match mode {
            PickerMode::Reinterpret if self.reinterpret.is_sensitive() => {
                self.reinterpret.set_active(true);
            }
            _ => self.convert.set_active(true),
        }
    }

    pub fn mode(&self) -> PickerMode {
        if self.reinterpret.is_active() {
            PickerMode::Reinterpret
        } else {
            PickerMode::Convert
        }
    }

    fn filter(&self) {
        let query = self.entry.text();
        for row in &self.rows {
            row.row
                .set_visible(palette::score(&query, &row.search).is_some() || query.is_empty());
        }
        self.list.invalidate_headers();
        if self.list.selected_row().is_none_or(|row| !row.is_visible()) {
            let first = self.rows.iter().find(|row| row.row.is_visible());
            self.list.select_row(first.map(|row| &row.row));
        }
    }

    fn select_current(&self) {
        let current = self.current.get();
        let row = self
            .rows
            .iter()
            .find(|row| current.is_some_and(|entry| std::ptr::eq(row.entry, entry)))
            .or(self.rows.first());
        if let Some(row) = row {
            self.list.select_row(Some(&row.row));
            self.scroll_to(&row.row);
        }
    }

    fn step(&self, delta: i32) {
        let visible: Vec<&gtk::ListBoxRow> = self
            .rows
            .iter()
            .map(|row| &row.row)
            .filter(|row| row.is_visible())
            .collect();
        let at = self
            .list
            .selected_row()
            .and_then(|selected| visible.iter().position(|row| **row == selected));
        let next = match at {
            Some(index) => (index as i32 + delta).clamp(0, visible.len() as i32 - 1) as usize,
            None => 0,
        };
        if let Some(row) = visible.get(next) {
            self.list.select_row(Some(*row));
            self.scroll_to(row);
        }
    }

    fn scroll_to(&self, row: &gtk::ListBoxRow) {
        if let Some(bounds) = row.compute_bounds(&self.list) {
            let adjustment = self.scrolled.vadjustment();
            adjustment.set_value(f64::from(bounds.y()) - adjustment.page_size() / 3.0);
        }
    }

    fn choose(&self, row: &gtk::ListBoxRow) {
        let Some(chosen) = self.rows.iter().find(|candidate| candidate.row == *row) else {
            return;
        };
        let mode = self.mode();
        self.popover.popdown();
        if let Some(on_chosen) = self.on_chosen.borrow().as_ref() {
            on_chosen(mode, chosen.entry);
        }
    }

    /// Chooses the entry with `id` in `mode`, as a click would (for the self-test).
    pub fn choose_id(&self, mode: PickerMode, id: &str) -> bool {
        let Some(row) = self
            .rows
            .iter()
            .find(|row| row.entry.id().eq_ignore_ascii_case(id))
        else {
            return false;
        };
        self.set_mode(mode);
        if self.mode() != mode {
            return false;
        }
        let row = row.row.clone();
        self.choose(&row);
        true
    }
}
