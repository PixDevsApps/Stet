//! Check for Updates (1.5): only when the user runs it, a worker asks GitHub which release is the
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
use stet_domain::actions::{Accelerator, ActionId};
use stet_domain::settings::Keymap;
use stet_domain::updates::{self, Outcome, UpdateError, Version};

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

/// What the dialog says once the check has an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub heading: String,
    pub body: String,
    /// The command that installs the newer release.
    pub command: Option<String>,
}

pub fn answer(running: Version, result: Result<Version, UpdateError>, keymap: &Keymap) -> Answer {
    let latest = match result {
        Ok(latest) => latest,
        Err(error) => {
            return Answer {
                heading: "Couldn't check for updates".to_owned(),
                body: failure(error).to_owned(),
                command: None,
            };
        }
    };
    match updates::compare(running, latest) {
        Outcome::Latest => Answer {
            heading: "You have the latest version".to_owned(),
            body: format!("Stet {running} is the newest release."),
            command: None,
        },
        Outcome::Ahead(latest) => Answer {
            heading: "You have the latest version".to_owned(),
            body: format!("This Stet, {running}, is newer than the latest release, {latest}."),
            command: None,
        },
        Outcome::Newer(latest) => {
            let quit = match keymap
                .keys(ActionId::Quit)
                .first()
                .and_then(|key| Accelerator::parse(key))
            {
                Some(key) => format!("press {} to quit Stet", key.display()),
                None => "choose File › Quit in Stet's menu".to_owned(),
            };
            Answer {
                heading: format!("Stet {latest} is available"),
                body: format!(
                    "You have {running}. To update, copy this command, paste it into a terminal \
                     and press Enter; it asks for your computer's password. Then {quit} and \
                     start it again: your tabs and unsaved text come back."
                ),
                command: Some(updates::update_command(latest)),
            }
        }
    }
}

fn failure(error: UpdateError) -> &'static str {
    match error {
        UpdateError::Offline => {
            "GitHub, where Stet's releases are, couldn't be reached. Check your internet \
             connection and try again."
        }
        UpdateError::Timeout => "GitHub didn't answer in time. Try again later.",
        UpdateError::Certificate => {
            "The connection to GitHub couldn't be verified, so its answer wasn't used. A network \
             that intercepts secure connections can cause this."
        }
        UpdateError::Busy => {
            "GitHub is limiting requests from your network right now. Try again in an hour."
        }
        UpdateError::NoRelease => "GitHub has no Stet release to compare with.",
        UpdateError::Unreadable => "GitHub's answer couldn't be read. Try again later.",
        UpdateError::NoCurl => {
            "Stet asks GitHub with the curl program, which isn't installed. Install it with \
             sudo pacman -S curl and try again."
        }
    }
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
            let answer = answer(running(), result, &keymap);
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

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn says_whether_this_is_the_latest() {
        let keymap = Keymap::default();
        let same = answer(v("1.5.0"), Ok(v("1.5.0")), &keymap);
        assert_eq!(same.heading, "You have the latest version");
        assert_eq!(same.body, "Stet 1.5.0 is the newest release.");
        assert_eq!(same.command, None);
        let ahead = answer(v("1.5.0"), Ok(v("1.3.4")), &keymap);
        assert_eq!(ahead.heading, "You have the latest version");
        assert!(
            ahead
                .body
                .contains("1.5.0, is newer than the latest release, 1.3.4"),
            "{}",
            ahead.body
        );
        assert_eq!(ahead.command, None);
    }

    #[test]
    fn a_newer_release_comes_with_its_command() {
        let newer = answer(v("1.5.0"), Ok(v("1.6.0")), &Keymap::default());
        assert_eq!(newer.heading, "Stet 1.6.0 is available");
        assert!(newer.body.starts_with("You have 1.5.0."), "{}", newer.body);
        assert!(
            newer.body.contains("press Ctrl+Alt+Q to quit Stet"),
            "{}",
            newer.body
        );
        assert_eq!(newer.command, Some(updates::update_command(v("1.6.0"))));
    }

    #[test]
    fn failures_say_what_to_do() {
        for error in [
            UpdateError::Offline,
            UpdateError::Timeout,
            UpdateError::Certificate,
            UpdateError::Busy,
            UpdateError::NoRelease,
            UpdateError::Unreadable,
            UpdateError::NoCurl,
        ] {
            let failed = answer(v("1.5.0"), Err(error), &Keymap::default());
            assert_eq!(failed.heading, "Couldn't check for updates");
            assert!(!failed.body.is_empty());
            assert_eq!(failed.command, None);
        }
    }

    #[test]
    fn the_running_version_reads() {
        assert_eq!(running().to_string(), env!("CARGO_PKG_VERSION"));
    }
}
