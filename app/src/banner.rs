//! Banners between the tab strip and the editor (DESIGN.md "Window layout"). They are built
//! from the same CSS nodes as AdwBanner (`banner > revealer > widget`), so libadwaita and the
//! theme style them alike, but take any number of buttons: AdwBanner has one, and a file that
//! changed on disk needs Reload and Keep Mine.

use gtk4 as gtk;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

/// What a banner is about. Banners stack in this order, the most urgent on top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BannerKind {
    Loading,
    Disk,
    /// What restoring the session found (M2): a file changed since its backup, or unsaved
    /// changes that could not be restored.
    Restore,
    ReadOnly,
    Binary,
    Lossy,
    Placeholder,
    MixedEol,
    LongLines,
    LargeFile,
    Highlighting,
}

/// A banner's button: its label and what it does.
pub struct BannerButton {
    pub label: String,
    pub on_click: Rc<dyn Fn()>,
}

impl BannerButton {
    pub fn new(label: impl Into<String>, on_click: impl Fn() + 'static) -> Self {
        Self {
            label: label.into(),
            on_click: Rc::new(on_click),
        }
    }
}

pub struct Banner {
    root: gtk::Box,
    revealer: gtk::Revealer,
    content: gtk::Box,
    label: gtk::Label,
    buttons: RefCell<Vec<gtk::Button>>,
}

impl Banner {
    pub fn new() -> Self {
        let label = gtk::Label::builder()
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::WordChar)
            .xalign(0.0)
            .hexpand(true)
            .selectable(true)
            .build();
        let content = gtk::Box::builder()
            .css_name("widget")
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .build();
        content.append(&label);
        let revealer = gtk::Revealer::builder()
            .child(&content)
            .transition_type(gtk::RevealerTransitionType::SlideDown)
            .transition_duration(0)
            .reveal_child(false)
            .build();
        let root = gtk::Box::builder()
            .css_name("banner")
            .orientation(gtk::Orientation::Vertical)
            .visible(false)
            .build();
        root.append(&revealer);
        Self {
            root,
            revealer,
            content,
            label,
            buttons: RefCell::new(Vec::new()),
        }
    }

    pub fn widget(&self) -> &gtk::Widget {
        self.root.upcast_ref()
    }

    /// Shows `title` with `buttons`, replacing what the banner showed before.
    pub fn show(&self, title: &str, buttons: Vec<BannerButton>) {
        self.label.set_label(title);
        for button in self.buttons.take() {
            self.content.remove(&button);
        }
        let mut kept = Vec::with_capacity(buttons.len());
        for BannerButton { label, on_click } in buttons {
            // A click leaves the focus in the editor; the keyboard still reaches the button.
            let button = gtk::Button::builder()
                .label(&label)
                .valign(gtk::Align::Center)
                .focus_on_click(false)
                .build();
            button.connect_clicked(move |_| on_click());
            self.content.append(&button);
            kept.push(button);
        }
        self.buttons.replace(kept);
        self.root.set_visible(true);
        self.revealer.set_reveal_child(true);
    }

    pub fn hide(&self) {
        self.revealer.set_reveal_child(false);
        self.root.set_visible(false);
    }

    pub fn is_shown(&self) -> bool {
        self.root.is_visible()
    }

    pub fn title(&self) -> String {
        self.label.label().to_string()
    }

    pub fn button_labels(&self) -> Vec<String> {
        self.buttons
            .borrow()
            .iter()
            .filter_map(|button| button.label().map(|label| label.to_string()))
            .collect()
    }

    /// Presses the button labelled `label` the way the keyboard does, with the focus on it
    /// (for the self-test).
    pub fn click(&self, label: &str) -> bool {
        let button = self
            .buttons
            .borrow()
            .iter()
            .find(|button| button.label().as_deref() == Some(label))
            .cloned();
        match button {
            Some(button) => {
                button.grab_focus();
                button.emit_clicked();
                true
            }
            None => false,
        }
    }
}
