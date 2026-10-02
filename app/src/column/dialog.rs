//! The Column Editor (Alt+C, Edit › Column Editor…): the dialog that writes the same text, or a
//! sequence of numbers, down a column: on the rectangle's lines in column mode, or from the
//! caret's line to the end of the text at the caret's column. The number fields are read in the
//! chosen format (Hex `B` is eleven). Errors show in the dialog, which stays open; the editor
//! gets the focus back when it closes. The values are kept for the next time while Stet runs.

use super::view::StetView;
use crate::window::Window;
use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;
use stet_domain::column::{Base, ColumnEditor, ColumnError, Leading, NumberSequence};

/// The dialog's values, kept between uses.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Choice {
    numbers: bool,
    text: String,
    initial: String,
    step: String,
    repeat: String,
    leading: Leading,
    base: Base,
    uppercase: bool,
}

impl Default for Choice {
    fn default() -> Self {
        Self {
            numbers: false,
            text: String::new(),
            initial: "1".to_owned(),
            step: "1".to_owned(),
            repeat: "1".to_owned(),
            leading: Leading::None,
            base: Base::Decimal,
            uppercase: true,
        }
    }
}

thread_local! {
    static OPEN: RefCell<Option<Rc<EditorDialog>>> = const { RefCell::new(None) };
    static LAST: RefCell<Option<Choice>> = const { RefCell::new(None) };
}

const LEADING: [(Leading, &str); 3] = [
    (Leading::None, "None"),
    (Leading::Zeros, "Zeros"),
    (Leading::Spaces, "Spaces"),
];

const BASES: [(Base, &str, u32); 4] = [
    (Base::Decimal, "_Dec", 10),
    (Base::Hex, "He_x", 16),
    (Base::Octal, "_Oct", 8),
    (Base::Binary, "_Bin", 2),
];

/// The open Column Editor.
pub struct EditorDialog {
    pub dialog: adw::Dialog,
    pub text_mode: gtk::CheckButton,
    pub number_mode: gtk::CheckButton,
    pub text: gtk::Entry,
    pub initial: gtk::Entry,
    pub step: gtk::Entry,
    pub repeat: gtk::Entry,
    pub leading: gtk::DropDown,
    pub bases: Vec<gtk::CheckButton>,
    pub uppercase: gtk::CheckButton,
    pub error: gtk::Label,
    pub ok: gtk::Button,
    pub cancel: gtk::Button,
    view: glib::WeakRef<StetView>,
}

/// The Column Editor on screen, if any.
pub fn current() -> Option<Rc<EditorDialog>> {
    OPEN.with(|open| open.borrow().clone())
}

/// Opens the Column Editor for `view`; a second Alt+C while it is open does nothing.
pub fn open(window: &Window, view: &StetView) {
    if current().is_some() {
        return;
    }
    view.close_burst();
    let editor = Rc::new(EditorDialog::new(view));
    editor.load(&LAST.with(|last| last.borrow().clone()).unwrap_or_default());
    editor.connect();
    let weak = Rc::downgrade(&window.inner);
    editor.dialog.connect_closed(move |_| {
        OPEN.with(|open| open.take());
        if let Some(inner) = weak.upgrade() {
            Window { inner }.restore_focus_later();
        }
    });
    OPEN.with(|open| open.replace(Some(editor.clone())));
    editor.dialog.present(Some(window.window()));
    editor.focus_first_field();
    // Opened from the palette, GTK moves the focus the closed palette had to the next
    // focusable widget after the next frame; taking it again before then cancels that, and
    // once mapped the dialog gives it to its focus widget.
    let weak = Rc::downgrade(&editor);
    glib::idle_add_local_full(glib::Priority::HIGH_IDLE, move || {
        if let Some(editor) = weak.upgrade() {
            editor.focus_first_field();
        }
        glib::ControlFlow::Break
    });
}

impl EditorDialog {
    fn new(view: &StetView) -> Self {
        let text_mode = gtk::CheckButton::with_mnemonic("_Text to insert");
        let number_mode = gtk::CheckButton::with_mnemonic("_Number to insert");
        number_mode.set_group(Some(&text_mode));
        let entry = || {
            gtk::Entry::builder()
                .activates_default(true)
                .hexpand(true)
                .build()
        };
        let text = entry();
        let initial = entry();
        let step = entry();
        let repeat = entry();
        let leading = gtk::DropDown::from_strings(&LEADING.map(|(_, label)| label));
        let bases: Vec<gtk::CheckButton> = BASES
            .iter()
            .map(|(_, label, _)| gtk::CheckButton::with_mnemonic(label))
            .collect();
        for base in &bases[1..] {
            base.set_group(Some(&bases[0]));
        }
        let uppercase = gtk::CheckButton::with_mnemonic("_Uppercase");
        // GTK 4.22 names a mnemonic check button with its underscore; screen readers get the
        // plain label (and the text field the label of its option).
        for (button, name) in [
            (&text_mode, "Text to insert"),
            (&number_mode, "Number to insert"),
            (&uppercase, "Uppercase"),
        ] {
            button.update_property(&[gtk::accessible::Property::Label(name)]);
        }
        for (button, (_, label, _)) in bases.iter().zip(BASES) {
            button.update_property(&[gtk::accessible::Property::Label(&label.replace('_', ""))]);
        }
        text.update_property(&[gtk::accessible::Property::Label("Text to insert")]);
        let error = gtk::Label::builder()
            .xalign(0.0)
            .wrap(true)
            .visible(false)
            .css_classes(["error"])
            .build();
        let ok = gtk::Button::builder()
            .label("OK")
            .css_classes(["suggested-action"])
            .build();
        let cancel = gtk::Button::builder().label("Cancel").build();

        let label = |text: &str, widget: &gtk::Widget| {
            gtk::Label::builder()
                .label(text)
                .use_underline(true)
                .mnemonic_widget(widget)
                .xalign(0.0)
                .build()
        };
        let grid = gtk::Grid::builder()
            .row_spacing(8)
            .column_spacing(12)
            .build();
        grid.attach(&text_mode, 0, 0, 1, 1);
        grid.attach(&text, 1, 0, 3, 1);
        grid.attach(&number_mode, 0, 1, 4, 1);
        let indent = |widget: &gtk::Label| {
            widget.set_margin_start(24);
        };
        let initial_label = label("_Initial number", initial.upcast_ref());
        indent(&initial_label);
        grid.attach(&initial_label, 0, 2, 1, 1);
        grid.attach(&initial, 1, 2, 1, 1);
        grid.attach(&label("Increase b_y", step.upcast_ref()), 2, 2, 1, 1);
        grid.attach(&step, 3, 2, 1, 1);
        let repeat_label = label("_Repeat", repeat.upcast_ref());
        indent(&repeat_label);
        grid.attach(&repeat_label, 0, 3, 1, 1);
        grid.attach(&repeat, 1, 3, 1, 1);
        grid.attach(&label("_Leading", leading.upcast_ref()), 2, 3, 1, 1);
        grid.attach(&leading, 3, 3, 1, 1);
        let format_label = gtk::Label::builder().label("Format").xalign(0.0).build();
        indent(&format_label);
        grid.attach(&format_label, 0, 4, 1, 1);
        let formats = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        for base in &bases {
            formats.append(base);
        }
        formats.append(&uppercase);
        grid.attach(&formats, 1, 4, 3, 1);

        let buttons = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .halign(gtk::Align::End)
            .build();
        buttons.append(&cancel);
        buttons.append(&ok);
        let content = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .margin_top(12)
            .margin_bottom(18)
            .margin_start(18)
            .margin_end(18)
            .build();
        content.append(&grid);
        content.append(&error);
        content.append(&buttons);
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        toolbar.set_content(Some(&content));
        let dialog = adw::Dialog::builder()
            .title("Column Editor")
            .content_width(520)
            .child(&toolbar)
            .build();
        dialog.add_css_class("stet-column-editor");
        dialog.set_default_widget(Some(&ok));
        Self {
            dialog,
            text_mode,
            number_mode,
            text,
            initial,
            step,
            repeat,
            leading,
            bases,
            uppercase,
            error,
            ok,
            cancel,
            view: view.downgrade(),
        }
    }

    fn connect(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        let sync = move || {
            if let Some(editor) = weak.upgrade() {
                editor.sync_sensitivity();
            }
        };
        for toggle in [&self.text_mode, &self.number_mode]
            .into_iter()
            .chain(&self.bases)
        {
            let sync = sync.clone();
            toggle.connect_toggled(move |_| sync());
        }
        let weak = Rc::downgrade(self);
        self.ok.connect_clicked(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.accept();
            }
        });
        let weak = Rc::downgrade(self);
        self.cancel.connect_clicked(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.dialog.close();
            }
        });
        self.sync_sensitivity();
    }

    /// Puts the focus in the text field, or in the initial number in number mode.
    fn focus_first_field(&self) {
        let first: &gtk::Entry = if self.number_mode.is_active() {
            &self.initial
        } else {
            &self.text
        };
        self.dialog.set_focus(Some(first));
        first.grab_focus();
    }

    fn sync_sensitivity(&self) {
        let numbers = self.number_mode.is_active();
        self.text.set_sensitive(!numbers);
        for widget in [
            self.initial.upcast_ref::<gtk::Widget>(),
            self.step.upcast_ref(),
            self.repeat.upcast_ref(),
            self.leading.upcast_ref(),
        ]
        .into_iter()
        .chain(self.bases.iter().map(|base| base.upcast_ref()))
        {
            widget.set_sensitive(numbers);
        }
        self.uppercase
            .set_sensitive(numbers && self.base() == Base::Hex);
    }

    fn base(&self) -> Base {
        BASES
            .iter()
            .zip(&self.bases)
            .find(|(_, button)| button.is_active())
            .map_or(Base::Decimal, |((base, _, _), _)| *base)
    }

    fn radix(&self) -> u32 {
        let base = self.base();
        BASES
            .iter()
            .find(|(candidate, _, _)| *candidate == base)
            .map_or(10, |(_, _, radix)| *radix)
    }

    fn load(&self, choice: &Choice) {
        self.text_mode.set_active(!choice.numbers);
        self.number_mode.set_active(choice.numbers);
        self.text.set_text(&choice.text);
        self.initial.set_text(&choice.initial);
        self.step.set_text(&choice.step);
        self.repeat.set_text(&choice.repeat);
        let leading = LEADING
            .iter()
            .position(|(leading, _)| *leading == choice.leading)
            .unwrap_or(0);
        self.leading.set_selected(leading as u32);
        for ((base, _, _), button) in BASES.iter().zip(&self.bases) {
            button.set_active(*base == choice.base);
        }
        self.uppercase.set_active(choice.uppercase);
    }

    fn choice(&self) -> Choice {
        Choice {
            numbers: self.number_mode.is_active(),
            text: self.text.text().to_string(),
            initial: self.initial.text().to_string(),
            step: self.step.text().to_string(),
            repeat: self.repeat.text().to_string(),
            leading: LEADING
                .get(self.leading.selected() as usize)
                .map_or(Leading::None, |(leading, _)| *leading),
            base: self.base(),
            uppercase: self.uppercase.is_active(),
        }
    }

    /// What the dialog asks for, or the message to show.
    fn read(&self) -> Result<ColumnEditor, String> {
        let choice = self.choice();
        if !choice.numbers {
            if choice.text.is_empty() {
                return Err("Type the text to insert.".to_owned());
            }
            return Ok(ColumnEditor::Text(choice.text));
        }
        let radix = self.radix();
        let format = match choice.base {
            Base::Decimal => "decimal",
            Base::Hex => "hexadecimal",
            Base::Octal => "octal",
            Base::Binary => "binary",
        };
        let number = |text: &str, field: &str| -> Result<i64, String> {
            parse_number(text, radix).ok_or_else(|| format!("“{field}” must be a {format} number."))
        };
        let start = number(&choice.initial, "Initial number")?;
        let step = number(&choice.step, "Increase by")?;
        let repeat = match choice.repeat.trim() {
            "" => 1,
            text => text
                .parse::<usize>()
                .map_err(|_| "“Repeat” must be a whole number.".to_owned())?,
        };
        Ok(ColumnEditor::Numbers(NumberSequence {
            start,
            step,
            repeat,
            base: choice.base,
            leading: choice.leading,
            width: 0,
            uppercase: choice.uppercase,
        }))
    }

    fn show_error(&self, message: &str) {
        self.error.set_label(message);
        self.error.set_visible(!message.is_empty());
    }

    /// OK: writes the column and closes, or shows why not.
    pub fn accept(&self) {
        let Some(view) = self.view.upgrade() else {
            self.dialog.close();
            return;
        };
        if !view.is_editable() {
            self.show_error("This document is read-only.");
            return;
        }
        let editor = match self.read() {
            Ok(editor) => editor,
            Err(message) => {
                self.show_error(&message);
                return;
            }
        };
        match view.column_editor(&editor) {
            Ok(()) => {
                LAST.with(|last| last.replace(Some(self.choice())));
                self.show_error("");
                self.dialog.close();
            }
            Err(ColumnError::LineBreak) => self.show_error("The text must fit on one line."),
            Err(ColumnError::Overflow) => {
                self.show_error("The numbers do not fit in 64 bits.");
            }
        }
    }

    /// The message on screen, if any.
    pub fn error_text(&self) -> Option<String> {
        self.error
            .is_visible()
            .then(|| self.error.label().to_string())
    }

    /// Chooses the format whose label (without the mnemonic) is `name`, case-insensitively.
    pub fn choose_base(&self, name: &str) -> bool {
        let found = BASES
            .iter()
            .zip(&self.bases)
            .find(|((_, label, _), _)| label.replace('_', "").eq_ignore_ascii_case(name));
        if let Some((_, button)) = found {
            button.set_active(true);
        }
        found.is_some()
    }

    /// Chooses the Leading entry labelled `name`, case-insensitively.
    pub fn choose_leading(&self, name: &str) -> bool {
        match LEADING
            .iter()
            .position(|(_, label)| label.eq_ignore_ascii_case(name))
        {
            Some(index) => {
                self.leading.set_selected(index as u32);
                true
            }
            None => false,
        }
    }
}

/// A number in `radix`, with an optional sign; empty is 0.
fn parse_number(text: &str, radix: u32) -> Option<i64> {
    let text = text.trim();
    if text.is_empty() {
        return Some(0);
    }
    let (negative, digits) = match text.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    if digits.is_empty() || digits.starts_with(['+', '-']) {
        return None;
    }
    let magnitude = u64::from_str_radix(digits, radix).ok()?;
    if negative {
        0i64.checked_sub_unsigned(magnitude)
    } else {
        i64::try_from(magnitude).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::parse_number;

    #[test]
    fn numbers_are_read_in_the_chosen_format() {
        assert_eq!(parse_number("", 10), Some(0));
        assert_eq!(parse_number(" 42 ", 10), Some(42));
        assert_eq!(parse_number("-7", 10), Some(-7));
        assert_eq!(parse_number("+7", 10), Some(7));
        assert_eq!(parse_number("B", 16), Some(11));
        assert_eq!(parse_number("ff", 16), Some(255));
        assert_eq!(parse_number("17", 8), Some(15));
        assert_eq!(parse_number("101", 2), Some(5));
        assert_eq!(parse_number("2", 2), None);
        assert_eq!(parse_number("x", 10), None);
        assert_eq!(parse_number("-", 10), None);
        assert_eq!(parse_number("--1", 10), None);
        assert_eq!(parse_number("-9223372036854775808", 10), Some(i64::MIN));
        assert_eq!(parse_number("9223372036854775808", 10), None);
    }
}
