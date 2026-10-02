//! Indentation (M5): every document's tab width and whether Tab inserts spaces. A new document
//! takes the settings for its language (`config.toml`, then Stet's defaults per language), an
//! opened file the indentation it already uses, and the status bar's "Spaces: 4" item opens a
//! popover to change it for the document. The exact tab stops of ADR-015 follow a width change
//! at the next layout.

use super::Window;
use super::toolbox::Reach;
use crate::editor::{EditorPage, IndentSource};
use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use stet_domain::indent::{self, Indentation};
use stet_domain::ops::lines::{self, Scope};

/// The widths the popover offers.
const WIDTHS: [u32; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

/// What the popover asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentChoice {
    Spaces(bool),
    Width(u32),
    Detect,
    ConvertToSpaces,
    ConvertToTabs,
}

type OnChosen = Box<dyn Fn(IndentChoice)>;

/// The status bar's indentation popover.
pub struct IndentPicker {
    pub popover: gtk::Popover,
    spaces: gtk::ToggleButton,
    tabs: gtk::ToggleButton,
    widths: Vec<(u32, gtk::ToggleButton)>,
    showing: Cell<bool>,
    on_chosen: RefCell<Option<OnChosen>>,
}

impl IndentPicker {
    pub fn new() -> Rc<Self> {
        let heading = |text: &str| {
            gtk::Label::builder()
                .label(text)
                .xalign(0.0)
                .css_classes(["caption-heading"])
                .build()
        };
        let linked = || {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
            row.add_css_class("linked");
            row
        };
        let spaces = gtk::ToggleButton::with_label("Spaces");
        let tabs = gtk::ToggleButton::with_label("Tabs");
        tabs.set_group(Some(&spaces));
        let kind = linked();
        kind.append(&spaces);
        kind.append(&tabs);
        let width_row = linked();
        let mut widths = Vec::new();
        for width in WIDTHS {
            let button = gtk::ToggleButton::with_label(&width.to_string());
            if let Some((_, first)) = widths.first() {
                button.set_group(Some(first));
            }
            width_row.append(&button);
            widths.push((width, button));
        }
        let detect = gtk::Button::with_label("Detect from Content");
        let to_spaces = gtk::Button::with_label("Convert Indentation to Spaces");
        let to_tabs = gtk::Button::with_label("Convert Indentation to Tabs");
        let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
        content.append(&heading("Indent with"));
        content.append(&kind);
        content.append(&heading("Tab width"));
        content.append(&width_row);
        content.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        content.append(&detect);
        content.append(&to_spaces);
        content.append(&to_tabs);
        let popover = gtk::Popover::builder().child(&content).build();
        popover.add_css_class("stet-indentation");
        let picker = Rc::new(Self {
            popover,
            spaces,
            tabs,
            widths,
            showing: Cell::new(false),
            on_chosen: RefCell::new(None),
        });
        let toggled = |picker: &Rc<Self>, button: &gtk::ToggleButton, choice: IndentChoice| {
            let weak = Rc::downgrade(picker);
            button.connect_toggled(move |button| {
                if let Some(picker) = weak.upgrade()
                    && button.is_active()
                    && !picker.showing.get()
                {
                    picker.emit(choice);
                }
            });
        };
        toggled(&picker, &picker.spaces, IndentChoice::Spaces(true));
        toggled(&picker, &picker.tabs, IndentChoice::Spaces(false));
        for (width, button) in &picker.widths {
            toggled(&picker, button, IndentChoice::Width(*width));
        }
        for (button, choice) in [
            (detect, IndentChoice::Detect),
            (to_spaces, IndentChoice::ConvertToSpaces),
            (to_tabs, IndentChoice::ConvertToTabs),
        ] {
            let weak = Rc::downgrade(&picker);
            button.connect_clicked(move |_| {
                if let Some(picker) = weak.upgrade() {
                    picker.popover.popdown();
                    picker.emit(choice);
                }
            });
        }
        picker
    }

    pub fn connect_chosen(&self, on_chosen: impl Fn(IndentChoice) + 'static) {
        self.on_chosen.replace(Some(Box::new(on_chosen)));
    }

    fn emit(&self, choice: IndentChoice) {
        if let Some(on_chosen) = self.on_chosen.borrow().as_ref() {
            on_chosen(choice);
        }
    }

    /// Shows `indentation` in the toggles without choosing anything.
    pub fn show(&self, indentation: Indentation) {
        self.showing.set(true);
        if indentation.insert_spaces {
            self.spaces.set_active(true);
        } else {
            self.tabs.set_active(true);
        }
        for (width, button) in &self.widths {
            button.set_active(*width == indentation.tab_width);
        }
        self.showing.set(false);
    }

    /// Chooses as a click would, for the self-test.
    pub fn choose(&self, choice: IndentChoice) {
        match choice {
            IndentChoice::Spaces(true) => self.spaces.set_active(true),
            IndentChoice::Spaces(false) => self.tabs.set_active(true),
            IndentChoice::Width(width) => match self.widths.iter().find(|(w, _)| *w == width) {
                Some((_, button)) => button.set_active(true),
                None => self.emit(choice),
            },
            other => {
                self.popover.popdown();
                self.emit(other);
            }
        }
    }

    /// The toggle that gets the focus when the popover opens.
    pub fn focus_widget(&self) -> gtk::Widget {
        if self.spaces.is_active() {
            self.spaces.clone().upcast()
        } else {
            self.tabs.clone().upcast()
        }
    }
}

impl Window {
    /// The status bar's popover shows the current document's indentation and changes it.
    pub(super) fn connect_indentation(&self) {
        let picker = &self.inner.indentation;
        picker.connect_chosen(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |choice| Window { inner }.indent_chosen(choice)
        ));
        picker.popover.connect_show(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |_| {
                let window = Window { inner };
                if let Some(page) = window.current_page() {
                    window.inner.indentation.show(page.indentation());
                }
            }
        ));
    }

    /// The indentation `page` gets from the settings, for its language.
    pub(super) fn default_indentation(&self, page: &EditorPage) -> Indentation {
        let language = page.language().map(|language| language.id().to_string());
        self.settings().indentation_for(language.as_deref())
    }

    /// Gives `page` the settings' indentation unless the file's own or the user's choice
    /// decides it.
    pub(super) fn refresh_indentation(&self, page: &EditorPage) {
        if page.indent_source() == IndentSource::Default {
            page.set_indentation(self.default_indentation(page), IndentSource::Default);
            if self.current_page().as_ref() == Some(page) {
                self.update_status();
            }
        }
    }

    /// After a file loaded: the indentation `detected` in it, or the settings'.
    pub(super) fn indentation_after_load(
        &self,
        page: &EditorPage,
        detected: Option<indent::Detected>,
    ) {
        if page.indent_source() == IndentSource::User {
            return;
        }
        let defaults = self.default_indentation(page);
        match detected.filter(|_| self.settings().detect_indentation) {
            Some(detected) => {
                page.set_indentation(defaults.with_detected(detected), IndentSource::Detected);
            }
            None => page.set_indentation(defaults, IndentSource::Default),
        }
    }

    /// Detect Indentation from Content, for the current document.
    pub(super) fn detect_indentation(&self) {
        let Some(page) = self.current_page() else {
            return;
        };
        let buffer = page.buffer();
        let mut end = buffer.start_iter();
        end.forward_lines(indent::DETECT_MAX_LINES as i32);
        let head = buffer.text(&buffer.start_iter(), &end, true);
        let defaults = self.default_indentation(&page);
        match indent::detect(indent::sample(&head)) {
            Some(detected) => {
                let indentation = defaults.with_detected(detected);
                page.set_indentation(indentation, IndentSource::Detected);
                self.toast(&format!("Indentation detected: {}", indentation.label()));
            }
            None => {
                page.set_indentation(defaults, IndentSource::Default);
                self.toast(&format!(
                    "No indentation found; using the settings: {}",
                    defaults.label()
                ));
            }
        }
        self.update_status();
    }

    /// The status bar popover's choice, for the current document.
    pub(super) fn indent_chosen(&self, choice: IndentChoice) {
        let Some(page) = self.current_page() else {
            return;
        };
        let mut indentation = page.indentation();
        match choice {
            IndentChoice::Spaces(spaces) => indentation.insert_spaces = spaces,
            IndentChoice::Width(width) => indentation.tab_width = width,
            IndentChoice::Detect => {
                self.detect_indentation();
                return;
            }
            IndentChoice::ConvertToSpaces | IndentChoice::ConvertToTabs => {
                let spaces = choice == IndentChoice::ConvertToSpaces;
                let width = indentation.tab_width as usize;
                indentation.insert_spaces = spaces;
                page.set_indentation(indentation, IndentSource::User);
                self.update_status();
                self.run_tool(
                    &page,
                    Reach::Document,
                    Box::new(move |text, target| {
                        Ok(if spaces {
                            lines::tabs_to_spaces(text, target, width, Scope::Leading)
                        } else {
                            lines::spaces_to_tabs(text, target, width, Scope::Leading)
                        })
                    }),
                    "Convert Indentation",
                );
                return;
            }
        }
        page.set_indentation(indentation, IndentSource::User);
        self.update_status();
    }

    /// Opens the status bar's indentation popover (the palette's "Indentation…").
    pub(super) fn choose_indentation(&self) {
        let Some(page) = self.current_page() else {
            return;
        };
        let picker = &self.inner.indentation;
        picker.show(page.indentation());
        self.inner.status.indentation.popup();
        let focus = picker.focus_widget();
        glib::idle_add_local_once(move || {
            focus.grab_focus();
        });
    }
}
