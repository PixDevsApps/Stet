//! The command palette (F1, Ctrl+Shift+P): every palette-visible action from the registry
//! with its menu path and keys, the recent files, and `:line[:col]` to go to a line.

use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gdk, glib};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use stet_domain::actions::ActionId;
use stet_domain::location::{LineCol, parse_line_col};
use stet_domain::palette;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Action(ActionId),
    OpenRecent(PathBuf),
    GoTo(LineCol),
}

#[derive(Debug, Clone)]
pub struct Item {
    pub label: String,
    pub detail: String,
    pub keys: String,
    pub command: Command,
}

impl Item {
    fn search_text(&self) -> String {
        format!("{} {}", self.label, self.detail)
    }
}

type OnCommand = Box<dyn Fn(Command)>;

pub struct Palette {
    pub popover: gtk::Popover,
    pub entry: gtk::SearchEntry,
    list: gtk::ListBox,
    scrolled: gtk::ScrolledWindow,
    items: RefCell<Vec<Item>>,
    shown: RefCell<Vec<Item>>,
    on_command: RefCell<Option<OnCommand>>,
}

impl Palette {
    pub fn new() -> Rc<Self> {
        let entry = gtk::SearchEntry::builder()
            .placeholder_text("Type a command, a recent file, or :line to go to a line")
            .search_delay(0)
            .hexpand(true)
            .build();
        entry.update_property(&[gtk::accessible::Property::Label("Command palette")]);
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Browse)
            .build();
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&list)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(420)
            .build();
        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.append(&entry);
        content.append(&scrolled);
        let popover = gtk::Popover::builder()
            .child(&content)
            .has_arrow(false)
            .position(gtk::PositionType::Bottom)
            .build();
        popover.add_css_class("stet-palette");
        popover.add_css_class("stet-mono");
        let palette = Rc::new(Self {
            popover,
            entry,
            list,
            scrolled,
            items: RefCell::new(Vec::new()),
            shown: RefCell::new(Vec::new()),
            on_command: RefCell::new(None),
        });
        palette.connect_signals();
        palette
    }

    pub fn connect_command(&self, on_command: impl Fn(Command) + 'static) {
        self.on_command.replace(Some(Box::new(on_command)));
    }

    /// Opens over `parent`, top-centre, with `items` and the query `text`.
    pub fn open(&self, parent: &impl IsA<gtk::Widget>, items: Vec<Item>, text: &str) {
        let parent = parent.as_ref();
        if self.popover.parent().as_ref() != Some(parent) {
            if self.popover.parent().is_some() {
                self.popover.unparent();
            }
            self.popover.set_parent(parent);
        }
        let width = parent.width().max(1);
        self.popover
            .set_pointing_to(Some(&gdk::Rectangle::new(width / 2, 8, 1, 1)));
        self.items.replace(items);
        self.entry.set_text(text);
        self.refresh();
        self.popover.popup();
        self.entry.grab_focus();
        self.entry.set_position(-1);
    }

    /// The labels of the rows shown, in order, for the self-test.
    pub fn shown_labels(&self) -> Vec<String> {
        self.shown
            .borrow()
            .iter()
            .map(|item| item.label.clone())
            .collect()
    }

    /// Runs the selected row, as Enter does.
    pub fn activate_selected(&self) {
        let index = self.list.selected_row().map(|row| row.index());
        let item = index.and_then(|index| self.shown.borrow().get(index as usize).cloned());
        if let Some(item) = item {
            self.run(item.command);
        }
    }

    fn connect_signals(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        self.entry.connect_search_changed(move |_| {
            if let Some(palette) = weak.upgrade() {
                palette.refresh();
            }
        });
        let weak = Rc::downgrade(self);
        self.entry.connect_activate(move |_| {
            if let Some(palette) = weak.upgrade() {
                palette.activate_selected();
            }
        });
        let weak = Rc::downgrade(self);
        self.entry.connect_stop_search(move |_| {
            if let Some(palette) = weak.upgrade() {
                palette.popover.popdown();
            }
        });
        let weak = Rc::downgrade(self);
        self.list.connect_row_activated(move |_, row| {
            if let Some(palette) = weak.upgrade() {
                let item = palette.shown.borrow().get(row.index() as usize).cloned();
                if let Some(item) = item {
                    palette.run(item.command);
                }
            }
        });
        let keys = gtk::EventControllerKey::new();
        let weak = Rc::downgrade(self);
        keys.connect_key_pressed(move |_, key, _, _| {
            let Some(palette) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            match key {
                gdk::Key::Down => palette.move_selection(1),
                gdk::Key::Up => palette.move_selection(-1),
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        });
        self.entry.add_controller(keys);
    }

    fn refresh(&self) {
        let query = self.entry.text().to_string();
        let shown: Vec<Item> = if query.trim_start().starts_with(':') {
            let target = parse_line_col(&query);
            vec![Item {
                label: match target {
                    Some(LineCol { line, column: None }) => format!("Go to line {line}"),
                    Some(LineCol {
                        line,
                        column: Some(column),
                    }) => format!("Go to line {line}, column {column}"),
                    None => "Type a line number, or line:column".to_owned(),
                },
                detail: String::new(),
                keys: String::new(),
                command: target.map_or(Command::Action(ActionId::GoToLine), Command::GoTo),
            }]
        } else {
            let items = self.items.borrow();
            let texts: Vec<String> = items.iter().map(Item::search_text).collect();
            palette::rank(&query, texts.iter().map(String::as_str))
                .into_iter()
                .map(|index| items[index].clone())
                .collect()
        };
        self.list.remove_all();
        for item in &shown {
            self.list.append(&row(item));
        }
        if let Some(first) = self.list.row_at_index(0) {
            self.list.select_row(Some(&first));
        }
        self.shown.replace(shown);
    }

    fn move_selection(&self, step: i32) {
        let count = self.shown.borrow().len() as i32;
        if count == 0 {
            return;
        }
        let current = self.list.selected_row().map_or(-1, |row| row.index());
        let next = (current + step).clamp(0, count - 1);
        if let Some(row) = self.list.row_at_index(next) {
            self.list.select_row(Some(&row));
            super::language::scroll_to(&self.scrolled, &self.list, &row);
        }
    }

    fn run(&self, command: Command) {
        if command == Command::Action(ActionId::GoToLine) {
            return;
        }
        self.popover.popdown();
        if let Some(on_command) = self.on_command.borrow().as_ref() {
            on_command(command);
        }
    }
}

fn row(item: &Item) -> gtk::ListBoxRow {
    let label = gtk::Label::builder().label(&item.label).xalign(0.0).build();
    let detail = gtk::Label::builder()
        .label(&item.detail)
        .xalign(0.0)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::Middle)
        .css_classes(["stet-detail"])
        .build();
    let keys = gtk::Label::builder()
        .label(&item.keys)
        .xalign(1.0)
        .css_classes(["stet-accel"])
        .build();
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    content.append(&label);
    content.append(&detail);
    content.append(&keys);
    let row = gtk::ListBoxRow::builder().child(&content).build();
    // A screen reader reads the row as one: the command, where it lives, its keys.
    let spoken: Vec<&str> = [
        item.label.as_str(),
        item.detail.as_str(),
        item.keys.as_str(),
    ]
    .into_iter()
    .filter(|part| !part.is_empty())
    .collect();
    row.update_property(&[gtk::accessible::Property::Label(&spoken.join(", "))]);
    row
}
