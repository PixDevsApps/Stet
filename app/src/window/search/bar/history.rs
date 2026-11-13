//! The fields' histories: the drop-down next to the find and replace fields, and Up and
//! Down walking a field's history in the field itself.

use super::{Field, FindBar};
use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{glib, pango};
use std::cell::RefCell;
use std::rc::Rc;
use stet_domain::search::{History, SearchHistory};

/// Called with the history entry the user picked.
type OnChosen = Rc<dyn Fn(String)>;

/// A history drop-down next to an entry.
pub(super) struct HistoryButton {
    pub(super) button: gtk::MenuButton,
    pub(super) list: gtk::ListBox,
    entries: Rc<RefCell<Vec<String>>>,
    chosen: Rc<RefCell<Option<OnChosen>>>,
}

impl HistoryButton {
    pub(super) fn new(tooltip: &str) -> Self {
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
                let entry = usize::try_from(row.index())
                    .ok()
                    .and_then(|index| entries.borrow().get(index).cloned());
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

impl FindBar {
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

    /// Up (`older`) and Down in a field walk its history.
    pub(super) fn recall(&self, field: Field, older: bool) -> bool {
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
                {
                    let _recalling = self.recalling.raise();
                    editable.set_text(&text);
                }
                editable.set_position(-1);
                true
            }
            None => false,
        }
    }

    /// A field's text changed: unless the history walk put it there, the walk ends.
    pub(super) fn field_changed(&self, field: Field) {
        if self.recalling.is_raised() {
            return;
        }
        let text = self.editable(field).text();
        let mut recalled = self.recalled.borrow_mut();
        if recalled[field.index()].as_deref() != Some(text.as_str()) {
            recalled[field.index()] = None;
            self.recalls.borrow_mut()[field.index()].reset();
        }
    }

    /// Lists `field`'s history in its drop-down; the entry picked goes into the field.
    pub(super) fn fill_history(self: &Rc<Self>, field: Field) {
        let entries = Self::history_of(&self.history.borrow(), field)
            .entries()
            .to_vec();
        let weak = Rc::downgrade(self);
        let chosen: OnChosen = Rc::new(move |text| {
            if let Some(bar) = weak.upgrade() {
                let editable = bar.editable(field);
                editable.set_text(&text);
                editable.grab_focus();
                editable.set_position(-1);
            }
        });
        let button = match field {
            Field::Replace => &self.replace_history,
            _ => &self.find_history,
        };
        button.fill(&entries, chosen);
    }
}
