//! Check for Updates: only when the user runs it, a worker asks GitHub which release is the
//! latest, and a dialog says whether this Stet is it. When a newer one is out, the dialog shows
//! the command that installs it, with a button that copies it; Stet never installs anything
//! itself.

use super::Window;
use gtk4 as gtk;
use gtk4::glib;
use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use std::cell::{Cell, RefCell};
use stet_domain::updates::{self, UpdateError, Version};

pub const CHECKING: &str = "Checking for updates…";
/// The button that copies the command.
pub const COPY_LABEL: &str = "Copy Command";

/// The check's state in a window.
#[derive(Default)]
pub struct UpdateState {
    /// A check's dialog is open.
    open: Cell<bool>,
    /// Where the self-test's release answers from, in place of GitHub.
    url: RefCell<Option<String>>,
}

/// The running Stet's version.
pub fn running() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_default()
}

impl Window {
    /// The self-test's release server, in place of GitHub.
    pub fn set_update_url(&self, url: Option<String>) {
        self.inner.updates.url.replace(url);
    }

    pub(super) fn check_for_updates(&self) {
        let state = &self.inner.updates;
        if state.open.replace(true) {
            return;
        }
        let url = state
            .url
            .borrow()
            .clone()
            .unwrap_or_else(updates::latest_release_url);
        let dialog = adw::AlertDialog::builder()
            .heading(CHECKING)
            .body("Asking GitHub for the latest release of Stet.")
            .close_response("close")
            .default_response("close")
            .build();
        dialog.add_response("close", "Cancel");
        dialog.set_extra_child(Some(&adw::Spinner::new()));
        dialog.connect_closed(glib::clone!(
            #[weak(rename_to = inner)]
            self.inner,
            move |dialog| {
                let window = Window { inner };
                window.inner.updates.open.set(false);
                let ours = window.inner.dialog.borrow().as_ref() == Some(dialog);
                if ours {
                    window.inner.dialog.replace(None);
                }
                window.restore_focus_later();
            }
        ));
        self.inner.dialog.replace(Some(dialog.clone()));
        dialog.present(Some(&self.inner.window));
        let keymap = self.keymap();
        let window = self.clone();
        self.spawn(async move {
            let result = crate::worker::run(move || stet_infrastructure::updates::latest(&url))
                .await
                .unwrap_or(Err(UpdateError::Offline));
            if window.inner.dialog.borrow().as_ref() != Some(&dialog) {
                return;
            }
            let answer = updates::answer(running(), result, &keymap);
            dialog.set_heading(Some(&answer.heading));
            dialog.set_body(&answer.body);
            dialog.set_response_label("close", "Close");
            match &answer.command {
                Some(command) => {
                    let (column, copy) = command_box(command);
                    dialog.set_extra_child(Some(&column));
                    // Not the command, which would show selected from end to end.
                    dialog.set_focus(Some(&copy));
                }
                None => dialog.set_extra_child(gtk::Widget::NONE),
            }
            dialog.upcast_ref::<gtk::Widget>().announce(
                &format!("{}. {}", answer.heading, answer.body),
                gtk::AccessibleAnnouncementPriority::Medium,
            );
        });
    }
}

/// The command, selectable in the editor's font, and a button that copies it.
fn command_box(command: &str) -> (gtk::Box, gtk::Button) {
    let label = gtk::Label::builder()
        .label(command)
        .selectable(true)
        .wrap(true)
        .wrap_mode(gtk::pango::WrapMode::Char)
        .xalign(0.0)
        .css_classes(["card", "stet-mono", "stet-update-command"])
        .build();
    // A break in the URL gets no hyphen, which would read as part of it.
    let attributes = gtk::pango::AttrList::new();
    attributes.insert(gtk::pango::AttrInt::new_insert_hyphens(false));
    label.set_attributes(Some(&attributes));
    let copy = gtk::Button::builder()
        .label(COPY_LABEL)
        .halign(gtk::Align::Center)
        .css_classes(["pill"])
        .build();
    let text = command.to_owned();
    copy.connect_clicked(move |button| {
        button.clipboard().set_text(&text);
        button.set_label("Copied");
        button.announce(
            "The update command is copied",
            gtk::AccessibleAnnouncementPriority::Medium,
        );
    });
    let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
    column.append(&label);
    column.append(&copy);
    (column, copy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_running_version_reads() {
        assert_eq!(running().to_string(), env!("CARGO_PKG_VERSION"));
    }
}
