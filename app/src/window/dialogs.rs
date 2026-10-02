//! In-window dialogs, only where a decision is needed (DESIGN.md), plus the file choosers.

use super::Window;
use crate::editor::EditorPage;
use crate::loader::FormatKind;
use gtk4 as gtk;
use gtk4::{gio, glib};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use stet_domain::text::{LONG_LINE_CHARS, LongestLine};
use stet_infrastructure::encoding::{Unmappable, status_label};

/// What About › Details says about Stet (1.3.2), as Pango markup.
const ABOUT_DETAILS: &str = "Stet is a fast, keyboard-first text and code editor for Omarchy, \
written in Rust with GTK 4, libadwaita and GtkSourceView 5.

It never loses your work: unsaved and untitled tabs come back after quitting, a crash or a \
restart. It follows your Omarchy theme and font as you change them, and it runs outside \
Omarchy too.

It has tabs and a split view, regex search and replace, Find and Replace in Files, column \
editing, bookmarks and marks, file comparison, line, case and comment tools, JSON and XML \
formatting and highlighting for 180 languages, and it keeps each file's encoding and line \
endings as they were.";

const WEBSITE: &str = "https://github.com/PixDevsApps/Stet";

/// The answer to "Save changes?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveChoice {
    Save,
    Discard,
    Cancel,
}

/// The answer to "this file has very long lines".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LongLineChoice {
    Formatted,
    DisplayBreaks,
    Cancel,
}

/// What the characters an encoding lacks were found for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Save,
    Convert,
}

/// The answer to "these characters can't be represented".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnmappableChoice {
    UseUtf8,
    /// Go to the character at this offset.
    GoTo(usize),
    Cancel,
}

/// How many unmappable characters the dialog lists.
const LISTED_UNMAPPABLE: usize = 20;

/// The label of the buttons that go to a character in the unmappable dialog.
pub const GO_TO_LABEL: &str = "Go To";

impl Window {
    /// Shows `dialog` until it is answered and returns the response id; the self-test finds
    /// it while it is open. Afterwards the editor gets the focus back, unless what the answer
    /// led to put it elsewhere.
    pub(super) async fn ask(&self, dialog: &adw::AlertDialog) -> String {
        self.inner.dialog.replace(Some(dialog.clone()));
        let answered = Rc::new(Cell::new(false));
        keep_focus_through_popover(dialog, answered.clone());
        let response = dialog.clone().choose_future(Some(&self.inner.window)).await;
        answered.set(true);
        self.inner.dialog.replace(None);
        self.restore_focus_later();
        response.to_string()
    }

    /// Asks a yes-or-no question; `accept` labels the button that goes ahead.
    pub async fn confirm(
        &self,
        heading: &str,
        body: &str,
        accept: &str,
        destructive: bool,
    ) -> bool {
        let dialog = adw::AlertDialog::new(Some(heading), Some(body));
        dialog.add_responses(&[("cancel", "_Cancel"), ("accept", accept)]);
        dialog.set_response_appearance(
            "accept",
            if destructive {
                adw::ResponseAppearance::Destructive
            } else {
                adw::ResponseAppearance::Suggested
            },
        );
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        self.ask(&dialog).await == "accept"
    }

    /// Reports something done that needs more than a toast.
    pub async fn show_message(&self, heading: &str, body: &str) {
        self.show_error(heading, body).await;
    }

    /// Tells about a failure that needs more than a toast.
    pub async fn show_error(&self, heading: &str, body: &str) {
        let dialog = adw::AlertDialog::new(Some(heading), Some(body));
        dialog.add_responses(&[("close", "_Close")]);
        dialog.set_default_response(Some("close"));
        dialog.set_close_response("close");
        self.ask(&dialog).await;
    }

    /// Asks how to open a file whose lines are too long to show as they are (S1).
    pub async fn ask_long_lines(
        &self,
        name: &str,
        longest: LongestLine,
        kind: Option<FormatKind>,
    ) -> LongLineChoice {
        let mut body = format!(
            "Line {} has {} characters. Lines over {} characters would freeze the editor, \
             so Stet doesn't show them as they are.",
            longest.line + 1,
            grouped(longest.chars),
            grouped(LONG_LINE_CHARS)
        );
        if let Some(kind) = kind {
            body.push_str(&format!(
                "\n\nOpen Formatted pretty-prints the {} on its lines; the text then differs \
                 from the file's bytes, and saving asks first.",
                kind.label()
            ));
        }
        body.push_str(
            "\n\nOpen Read-Only breaks the long lines for display; the text can't be saved.",
        );
        let dialog =
            adw::AlertDialog::new(Some(&format!("“{name}” has very long lines")), Some(&body));
        dialog.add_responses(&[("cancel", "_Cancel"), ("breaks", "Open _Read-Only")]);
        if let Some(kind) = kind {
            dialog.add_response("formatted", &format!("Open _Formatted as {}", kind.label()));
            dialog.set_response_appearance("formatted", adw::ResponseAppearance::Suggested);
            dialog.set_default_response(Some("formatted"));
        } else {
            dialog.set_default_response(Some("breaks"));
        }
        dialog.set_close_response("cancel");
        match self.ask(&dialog).await.as_str() {
            "formatted" if kind.is_some() => LongLineChoice::Formatted,
            "breaks" => LongLineChoice::DisplayBreaks,
            _ => LongLineChoice::Cancel,
        }
    }

    /// Lists the characters the target encoding lacks, each with a Go To button, and offers
    /// UTF-8, which has them all.
    pub async fn ask_unmappable(
        &self,
        page: &EditorPage,
        error: &Unmappable,
        bom: bool,
        purpose: Purpose,
    ) -> UnmappableChoice {
        let name = page.name();
        let target = status_label(error.encoding, bom);
        let heading = match purpose {
            Purpose::Save => format!("“{name}” can't be saved as {target}"),
            Purpose::Convert => format!("“{name}” can't be converted to {target}"),
        };
        let count = if error.count == 1 {
            "1 character has".to_owned()
        } else {
            format!("{} characters have", grouped(error.count))
        };
        let body = format!(
            "{count} no {target} equivalent. Go to one to change it, or use UTF-8, which can \
             represent every character."
        );
        let dialog = adw::AlertDialog::new(Some(&heading), Some(&body));
        let chosen = Rc::new(Cell::new(None));
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        let buffer = page.buffer();
        for &(offset, character) in error.chars.iter().take(LISTED_UNMAPPABLE) {
            let iter = buffer.iter_at_offset(offset as i32);
            let text = format!(
                "Line {}, column {}: {character} (U+{:04X})",
                iter.line() + 1,
                iter.line_offset() + 1,
                u32::from(character)
            );
            let label = gtk::Label::builder()
                .label(&text)
                .xalign(0.0)
                .hexpand(true)
                .build();
            let button = gtk::Button::builder()
                .label(GO_TO_LABEL)
                .valign(gtk::Align::Center)
                .build();
            button.add_css_class("flat");
            button.connect_clicked(glib::clone!(
                #[strong]
                chosen,
                #[weak]
                dialog,
                move |_| {
                    chosen.set(Some(offset));
                    dialog.close();
                }
            ));
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            row.set_margin_start(6);
            row.set_margin_end(6);
            row.append(&label);
            row.append(&button);
            list.append(&row);
        }
        let mut extra: gtk::Widget = list.upcast();
        if error.count > LISTED_UNMAPPABLE.min(error.chars.len()) {
            let shown = LISTED_UNMAPPABLE.min(error.chars.len());
            let more = gtk::Label::builder()
                .label(format!("and {} more", grouped(error.count - shown)))
                .xalign(0.0)
                .css_classes(["dim-label"])
                .build();
            let column = gtk::Box::new(gtk::Orientation::Vertical, 6);
            column.append(&extra);
            column.append(&more);
            extra = column.upcast();
        }
        let scrolled = gtk::ScrolledWindow::builder()
            .child(&extra)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .max_content_height(240)
            .build();
        dialog.set_extra_child(Some(&scrolled));
        let use_utf8 = match purpose {
            Purpose::Save => "Save as _UTF-8",
            Purpose::Convert => "Convert to _UTF-8",
        };
        dialog.add_responses(&[("cancel", "_Cancel"), ("utf8", use_utf8)]);
        dialog.set_response_appearance("utf8", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("utf8"));
        dialog.set_close_response("cancel");
        let response = self.ask(&dialog).await;
        match (chosen.get(), response.as_str()) {
            (Some(offset), _) => UnmappableChoice::GoTo(offset),
            (None, "utf8") => UnmappableChoice::UseUtf8,
            _ => UnmappableChoice::Cancel,
        }
    }

    /// Asks whether to save the documents named in `names` before they close.
    pub async fn ask_save_changes(&self, names: &[String]) -> SaveChoice {
        let (heading, body) = match names {
            [name] => (
                format!("Save changes to “{name}”?"),
                "Unsaved changes are lost if you don't save them.".to_owned(),
            ),
            names => (
                format!("Save changes to {} documents?", names.len()),
                format!(
                    "{}\n\nUnsaved changes are lost if you don't save them.",
                    names.join("\n")
                ),
            ),
        };
        let dialog = adw::AlertDialog::new(Some(&heading), Some(&body));
        let (discard, save) = if names.len() == 1 {
            ("_Discard", "_Save")
        } else {
            ("_Discard All", "_Save All")
        };
        dialog.add_responses(&[("cancel", "_Cancel"), ("discard", discard), ("save", save)]);
        dialog.set_response_appearance("discard", adw::ResponseAppearance::Destructive);
        dialog.set_response_appearance("save", adw::ResponseAppearance::Suggested);
        dialog.set_default_response(Some("save"));
        dialog.set_close_response("cancel");
        match self.ask(&dialog).await.as_str() {
            "save" => SaveChoice::Save,
            "discard" => SaveChoice::Discard,
            _ => SaveChoice::Cancel,
        }
    }

    /// The alert dialog on screen, if any; the self-test answers it.
    pub fn open_dialog(&self) -> Option<adw::AlertDialog> {
        self.inner.dialog.borrow().clone()
    }

    pub async fn choose_files_to_open(&self, folder: Option<&Path>) -> Vec<PathBuf> {
        let dialog = gtk::FileDialog::builder().title("Open").modal(true).build();
        if let Some(folder) = folder {
            dialog.set_initial_folder(Some(&gio::File::for_path(folder)));
        }
        match dialog.open_multiple_future(Some(&self.inner.window)).await {
            Ok(files) => (0..files.n_items())
                .filter_map(|index| files.item(index).and_downcast::<gio::File>())
                .filter_map(|file| file.path())
                .collect(),
            Err(error) => {
                if !error.matches(gtk::DialogError::Dismissed) {
                    self.toast(&format!("Could not open the file chooser: {error}"));
                }
                Vec::new()
            }
        }
    }

    pub async fn choose_save_path(&self, current: Option<&Path>, name: &str) -> Option<PathBuf> {
        let dialog = gtk::FileDialog::builder()
            .title("Save As")
            .modal(true)
            .build();
        match current {
            Some(path) => dialog.set_initial_file(Some(&gio::File::for_path(path))),
            None => dialog.set_initial_name(Some(name)),
        }
        match dialog.save_future(Some(&self.inner.window)).await {
            Ok(file) => file.path(),
            Err(error) => {
                if !error.matches(gtk::DialogError::Dismissed) {
                    self.toast(&format!("Could not open the file chooser: {error}"));
                }
                None
            }
        }
    }

    pub fn show_about(&self) {
        let about = adw::AboutDialog::builder()
            .application_name("Stet")
            .application_icon(crate::config::ICON_NAME)
            .version(env!("CARGO_PKG_VERSION"))
            .comments(ABOUT_DETAILS)
            .website(WEBSITE)
            .issue_url(format!("{WEBSITE}/issues"))
            .developer_name("Stet contributors")
            .copyright("© 2026 Stet contributors")
            .license_type(gtk::License::MitX11)
            .build();
        about.connect_closed(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |_| Window { inner }.restore_focus_later()
        ));
        about.present(Some(&self.inner.window));
    }

    /// Shows `message` as a toast, as plain text: it can hold file names and keys such as
    /// `<Control>d`, which libadwaita would read as markup.
    pub fn toast(&self, message: &str) {
        let toast = adw::Toast::builder()
            .title(message)
            .use_markup(false)
            .timeout(4)
            .build();
        self.inner.toasts.add_toast(toast);
        self.inner.last_toast.replace(message.to_owned());
    }

    /// The text of the last toast, for the self-test.
    pub fn last_toast(&self) -> String {
        self.inner.last_toast.borrow().clone()
    }
}

/// `12345678` as `12,345,678`.
pub fn grouped(number: usize) -> String {
    let digits = number.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// Clicks the dialog's button for `response`, as a user would.
pub fn click_response(dialog: &adw::AlertDialog, response: &str) -> bool {
    let label = dialog.response_label(response);
    find_button(dialog.upcast_ref(), &label).is_some_and(|button| {
        button.emit_clicked();
        true
    })
}

fn find_button(widget: &gtk::Widget, label: &str) -> Option<gtk::Button> {
    if let Some(button) = widget.downcast_ref::<gtk::Button>()
        && button.label().as_deref() == Some(label)
    {
        return Some(button.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(found) = find_button(&current, label) {
            return Some(found);
        }
        child = current.next_sibling();
    }
    None
}

/// A dialog opened from a popover menu (the menu, the tab menu, the editor's) loses the focus
/// widget it was given when the popover finishes closing, live on Hyprland: libadwaita then
/// focuses the default response, so a name typed into Rename… went nowhere. Once the
/// dialog is up and the popover gone, the widget it was given gets the focus back, unless the
/// focus is in it already, and unless the dialog has been answered (it may still be on
/// screen, closing).
fn keep_focus_through_popover(dialog: &adw::AlertDialog, answered: Rc<Cell<bool>>) {
    let Some(wanted) = dialog.focus() else {
        return;
    };
    let dialog = dialog.clone();
    glib::spawn_future_local(async move {
        for _ in 0..200 {
            if dialog.is_mapped() {
                break;
            }
            glib::timeout_future(std::time::Duration::from_millis(5)).await;
        }
        glib::timeout_future(std::time::Duration::from_millis(80)).await;
        if answered.get() || !dialog.is_mapped() {
            return;
        }
        let inside = dialog
            .focus()
            .is_some_and(|focus| focus == wanted || focus.is_ancestor(&wanted));
        if !inside {
            wanted.grab_focus();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::ABOUT_DETAILS;
    use gtk4::pango;

    #[test]
    fn the_about_details_are_markup_and_describe_stet() {
        let (_, text, _) = pango::parse_markup(ABOUT_DETAILS, '\0').expect("valid markup");
        assert_eq!(
            text.lines().next(),
            Some(
                "Stet is a fast, keyboard-first text and code editor for Omarchy, written in \
                 Rust with GTK 4, libadwaita and GtkSourceView 5."
            )
        );
    }
}
