//! Self-test commands for the window's dialogs: answering them, filling their fields, and
//! what they say.

use super::registry::{Command, commands};
use super::script::boolean;
use super::{Harness, StepResult, equal, labels_in, number};
use crate::window::dialogs::{GO_TO_LABEL, click_button, click_response};
use gtk4 as gtk;
use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;

pub(super) const COMMANDS: &[Command] = commands![
    "respond" (1, 1) => async respond,
    "dialog-goto" (1, 1) => async dialog_goto,
    "dialog-set" (2, 2) => dialog_set,
    "dialog-enter" (1, 1) => async dialog_enter,
    "click-dialog-button" (1, 1) => async click_dialog_button,
    "assert-dialog" (1, 1) => assert_dialog,
    "assert-dialog-text" (1, 1) => assert_dialog_text,
    "assert-dialog-lacks" (1, 1) => assert_dialog_lacks,
    "assert-dialog-selection" (2, 2) => assert_dialog_selection,
];

impl Harness {
    fn dialog(&self) -> Result<adw::AlertDialog, String> {
        self.window
            .open_dialog()
            .ok_or_else(|| "no dialog is open".to_owned())
    }

    /// `respond <response id>`: clicks the open dialog's button with that response (`save`,
    /// `discard`, `cancel`) and waits for its work.
    async fn respond(&self, args: &[String]) -> StepResult {
        let dialog = self.dialog()?;
        if !click_response(&dialog, &args[0]) {
            return Err(format!("the dialog has no {} button", args[0]));
        }
        self.wait_idle().await
    }

    /// `dialog-goto <n>`: clicks the n-th Go To button of the open dialog (the unmappable
    /// characters' list) and waits for its work.
    async fn dialog_goto(&self, args: &[String]) -> StepResult {
        let dialog = self.dialog()?;
        let index = number(&args[0])?;
        let buttons = buttons_labelled(dialog.upcast_ref(), GO_TO_LABEL);
        let button = buttons
            .get(index.saturating_sub(1))
            .ok_or_else(|| format!("the dialog has {} Go To buttons", buttons.len()))?;
        button.emit_clicked();
        self.wait_idle().await
    }

    /// `dialog-set <title> <value>`: fills a field of the open dialog, found by its title: an
    /// entry row's text, a combo row's choice or a switch row's state.
    fn dialog_set(&self, args: &[String]) -> StepResult {
        let (title, value) = (args[0].as_str(), args[1].as_str());
        let dialog = self.dialog()?;
        let row = find_row(dialog.upcast_ref(), title)
            .ok_or_else(|| format!("the dialog has no field {title:?}"))?;
        if let Some(entry) = row.downcast_ref::<adw::EntryRow>() {
            entry.set_text(value);
            return Ok(());
        }
        if let Some(switch) = row.downcast_ref::<adw::SwitchRow>() {
            switch.set_active(boolean(value)?);
            return Ok(());
        }
        if let Some(combo) = row.downcast_ref::<adw::ComboRow>() {
            let model = combo
                .model()
                .and_downcast::<gtk::StringList>()
                .ok_or("the choices are not a string list")?;
            let index = (0..model.n_items())
                .find(|index| model.string(*index).is_some_and(|text| text == value))
                .ok_or_else(|| format!("{title} has no choice {value:?}"))?;
            combo.set_selected(index);
            return Ok(());
        }
        Err(format!("{title} is not a field the harness can fill"))
    }

    /// `dialog-enter <title>`: Enter in the open dialog's entry row with that title, as GtkText
    /// activates it, and waits for its work.
    async fn dialog_enter(&self, args: &[String]) -> StepResult {
        let dialog = self.dialog()?;
        let text = find_row(dialog.upcast_ref(), &args[0])
            .and_then(|row| find_text(&row))
            .ok_or_else(|| format!("the dialog has no entry {:?}", args[0]))?;
        text.emit_by_name::<()>("activate", &[]);
        self.wait_idle().await
    }

    /// `click-dialog-button <label>`: clicks the button with that label in the open alert
    /// dialog's own content (Copy Command).
    async fn click_dialog_button(&self, args: &[String]) -> StepResult {
        let clicked = self
            .window
            .open_dialog()
            .is_some_and(|dialog| click_button(&dialog, &args[0]));
        if clicked {
            self.wait_until_settled().await;
            Ok(())
        } else {
            Err(format!("no open dialog has a button {}", args[0]))
        }
    }

    /// The text of the open dialog: a question's heading, body and labels, or the labels of
    /// another dialog the window shows, such as About.
    fn dialog_text(&self) -> Result<String, String> {
        if let Some(dialog) = self.window.open_dialog() {
            return Ok(format!(
                "{}\n{}\n{}",
                dialog.heading().unwrap_or_default(),
                dialog.body(),
                labels_in(dialog.upcast_ref()).join("\n")
            ));
        }
        self.window
            .window()
            .visible_dialog()
            .map(|dialog| labels_in(dialog.upcast_ref()).join("\n"))
            .ok_or_else(|| "no dialog is open".to_owned())
    }

    /// `assert-dialog true|false`: whether a question dialog is open.
    fn assert_dialog(&self, args: &[String]) -> StepResult {
        equal(
            "dialog open",
            self.window.open_dialog().is_some(),
            boolean(&args[0])?,
        )
    }

    /// `assert-dialog-text <text>`: the open dialog has the text in its heading, body or
    /// labels.
    fn assert_dialog_text(&self, args: &[String]) -> StepResult {
        let text = self.dialog_text()?;
        if text.contains(args[0].as_str()) {
            Ok(())
        } else {
            Err(format!("the dialog {text:?} lacks {:?}", args[0]))
        }
    }

    /// `assert-dialog-lacks <text>`: the open dialog does not have the text.
    fn assert_dialog_lacks(&self, args: &[String]) -> StepResult {
        let text = self.dialog_text()?;
        if text.contains(args[0].as_str()) {
            Err(format!("the dialog {text:?} has {:?}", args[0]))
        } else {
            Ok(())
        }
    }

    /// `assert-dialog-selection <title> <text>`: the selected text in the open dialog's entry
    /// row with that title.
    fn assert_dialog_selection(&self, args: &[String]) -> StepResult {
        let dialog = self.dialog()?;
        let entry = find_row(dialog.upcast_ref(), &args[0])
            .and_then(|row| row.downcast::<adw::EntryRow>().ok())
            .ok_or_else(|| format!("the dialog has no entry {:?}", args[0]))?;
        equal(
            &format!("the selection in {}", args[0]),
            entry
                .selection_bounds()
                .map(|(start, end)| entry.chars(start, end).to_string())
                .unwrap_or_default(),
            args[1].clone(),
        )
    }
}

/// Every button under `widget` labelled `label`, in tree order.
fn buttons_labelled(widget: &gtk::Widget, label: &str) -> Vec<gtk::Button> {
    let mut found = Vec::new();
    if let Some(button) = widget.downcast_ref::<gtk::Button>()
        && button.label().as_deref() == Some(label)
    {
        found.push(button.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        found.extend(buttons_labelled(&current, label));
        child = current.next_sibling();
    }
    found
}

/// The row titled `title` under `widget`.
fn find_row(widget: &gtk::Widget, title: &str) -> Option<gtk::Widget> {
    if let Some(row) = widget.downcast_ref::<adw::PreferencesRow>()
        && row.title() == title
    {
        return Some(widget.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(found) = find_row(&current, title) {
            return Some(found);
        }
        child = current.next_sibling();
    }
    None
}

/// The GtkText inside an entry row.
fn find_text(widget: &gtk::Widget) -> Option<gtk::Text> {
    if let Some(text) = widget.downcast_ref::<gtk::Text>() {
        return Some(text.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(found) = find_text(&current) {
            return Some(found);
        }
        child = current.next_sibling();
    }
    None
}
