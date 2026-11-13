//! The banners above a page's text (DESIGN.md "Window layout"): one widget per kind, made on
//! first use and stacked in [`BannerKind`] order. What they say is decided elsewhere; the page
//! only shows them.
//!
//! A banner is built from the same CSS nodes as AdwBanner (`banner > revealer > widget`), so
//! libadwaita and the theme style them alike, but takes any number of buttons: AdwBanner has
//! one, and a file that changed on disk needs Reload and Keep Mine.

use super::page::EditorPage;
use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::subclass::prelude::*;
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;
use stet_domain::document::banners::BannerKind;

/// A banner's button: its label and what it does.
pub struct BannerButton {
    pub label: String,
    pub on_click: Rc<dyn Fn()>,
}

impl fmt::Debug for BannerButton {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BannerButton")
            .field("label", &self.label)
            .finish_non_exhaustive()
    }
}

impl BannerButton {
    pub fn new(label: impl Into<String>, on_click: impl Fn() + 'static) -> Self {
        Self {
            label: label.into(),
            on_click: Rc::new(on_click),
        }
    }
}

#[derive(Debug)]
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

    #[cfg(feature = "selftest")]
    pub fn is_shown(&self) -> bool {
        self.root.is_visible()
    }

    #[cfg(feature = "selftest")]
    pub fn title(&self) -> String {
        self.label.label().to_string()
    }

    #[cfg(feature = "selftest")]
    pub fn button_labels(&self) -> Vec<String> {
        self.buttons
            .borrow()
            .iter()
            .filter_map(|button| button.label().map(|label| label.to_string()))
            .collect()
    }

    /// Presses the button labelled `label` the way the keyboard does, with the focus on it
    /// (for the self-test).
    #[cfg(feature = "selftest")]
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

impl EditorPage {
    /// Shows the banner of `kind` with `title` and `buttons`, in its place in the stack.
    pub fn show_banner(&self, kind: BannerKind, title: &str, buttons: Vec<BannerButton>) {
        self.banner(kind).show(title, buttons);
    }

    pub fn hide_banner(&self, kind: BannerKind) {
        if let Some(banner) = self.imp().banners.borrow().get(&kind) {
            banner.hide();
        }
    }

    fn banner(&self, kind: BannerKind) -> Rc<Banner> {
        let imp = self.imp();
        if let Some(banner) = imp.banners.borrow().get(&kind) {
            return banner.clone();
        }
        let banner = Rc::new(Banner::new());
        let container = imp.banner_box.get().expect("constructed");
        let before = imp
            .banners
            .borrow()
            .range(..kind)
            .next_back()
            .map(|(_, banner)| banner.widget().clone());
        container.insert_child_after(banner.widget(), before.as_ref());
        imp.banners.borrow_mut().insert(kind, banner.clone());
        banner
    }

    /// The titles of the banners on screen, top to bottom.
    #[cfg(feature = "selftest")]
    pub fn banner_texts(&self) -> Vec<String> {
        self.imp()
            .banners
            .borrow()
            .values()
            .filter(|banner| banner.is_shown())
            .map(|banner| banner.title())
            .collect()
    }

    /// Clicks the button labelled `label` on a banner on screen.
    #[cfg(feature = "selftest")]
    pub fn click_banner(&self, label: &str) -> bool {
        let banners: Vec<Rc<Banner>> = self.imp().banners.borrow().values().cloned().collect();
        banners
            .iter()
            .filter(|banner| banner.is_shown())
            .any(|banner| banner.click(label))
    }

    /// The labels of the buttons on the banners on screen.
    #[cfg(feature = "selftest")]
    pub fn banner_buttons(&self) -> Vec<String> {
        self.imp()
            .banners
            .borrow()
            .values()
            .filter(|banner| banner.is_shown())
            .flat_map(|banner| banner.button_labels())
            .collect()
    }
}
