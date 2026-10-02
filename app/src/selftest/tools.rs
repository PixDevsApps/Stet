//! Self-test commands for M5: the text toolbox, the editors' shortcuts and the built-in
//! bindings ADR-009 overrides, `config.toml` and `keys.toml`, the context menus, indentation,
//! the document map, the Keyboard Shortcuts overview and Set as Default Editor
//! (docs/TESTING.md).

use super::script::{Step, boolean};
use super::{Harness, StepResult, equal, labels_in, number};
use crate::actions::EDITOR_SHORTCUTS;
use crate::window::IndentChoice;
use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gio, glib};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::path::Path;
use stet_domain::actions::{ActionId, KeyScope, OVERRIDDEN_BUILTINS};

/// The commands and their argument counts (minimum, maximum), as in `script::COMMANDS`.
pub const COMMANDS: &[(&str, usize, usize)] = &[
    ("sandbox-home", 0, 0),
    ("set-text", 1, 1),
    ("action-nowait", 1, 1),
    ("wait-tool", 0, 0),
    ("record-launches", 0, 0),
    ("editor-key", 1, 1),
    ("dialog-set", 2, 2),
    ("dialog-enter", 1, 1),
    ("indent-choose", 1, 1),
    ("tab-menu", 1, 1),
    ("tab-menu-close", 0, 0),
    ("tab-press", 3, 3),
    ("editor-menu", 0, 0),
    ("editor-menu-close", 0, 0),
    ("close-shortcuts", 0, 0),
    ("assert-launched", 1, 1),
    ("assert-editor-key", 2, 2),
    ("assert-editor-keys", 0, 0),
    ("assert-tool-stats", 2, 2),
    ("assert-tool-pieces", 1, 1),
    ("assert-indentation", 1, 1),
    ("assert-map", 1, 1),
    ("assert-tab-menu", 1, 1),
    ("assert-editor-menu", 1, 1),
    ("assert-shortcuts", 1, 1),
    ("assert-preferences-error", 2, 2),
    ("assert-editor-font", 1, 1),
    ("assert-file-contains", 2, 2),
    ("assert-dialog-selection", 2, 2),
    ("assert-line", 2, 2),
];

impl Harness {
    /// Runs an M5 command; `None` when `step` is not one.
    pub(super) async fn tools_step(&self, step: &Step) -> Option<StepResult> {
        let arg = |index: usize| step.args[index].as_str();
        Some(match step.command.as_str() {
            // Applied before the app started (see `run`).
            "sandbox-home" => Ok(()),
            // A new text with no undo history and the caret at the start.
            "set-text" => match self.page() {
                Ok(page) => {
                    let buffer = page.buffer();
                    buffer.begin_irreversible_action();
                    buffer.set_text(arg(0));
                    buffer.end_irreversible_action();
                    buffer.place_cursor(&buffer.start_iter());
                    Ok(())
                }
                Err(error) => Err(error),
            },
            "wait-tool" => self.wait_tool().await,
            // Starts an action and goes on at once, so the next step runs while its worker
            // does (`action` lets the main loop settle first).
            "action-nowait" => self.activate(arg(0), None),
            "record-launches" => {
                self.window.shared().record_launches();
                Ok(())
            }
            "editor-key" => {
                let pressed = self.editor_key(arg(0));
                self.wait_until_settled().await;
                pressed
            }
            "dialog-set" => self.dialog_set(arg(0), arg(1)),
            // Enter in a dialog's entry row, as GtkText activates it.
            "dialog-enter" => match self.window.open_dialog() {
                Some(dialog) => {
                    match find_row(dialog.upcast_ref(), arg(0)).and_then(|row| find_text(&row)) {
                        Some(text) => {
                            text.emit_by_name::<()>("activate", &[]);
                            self.wait_idle().await
                        }
                        None => Err(format!("the dialog has no entry {:?}", arg(0))),
                    }
                }
                None => Err("no dialog is open".to_owned()),
            },
            "indent-choose" => match indent_choice(arg(0)) {
                Ok(choice) => {
                    self.window.inner.indentation.choose(choice);
                    self.wait_idle().await
                }
                Err(error) => Err(error),
            },
            "tab-menu" => match number(arg(0)) {
                Ok(index) => match self.window.tab_at(index) {
                    Some((tabs, page)) => {
                        tabs.emit_by_name::<()>("setup-menu", &[&Some(page)]);
                        Ok(())
                    }
                    None => Err(format!("there is no tab {index}")),
                },
                Err(error) => Err(error),
            },
            "tab-press" => self.tab_press(arg(0), arg(1), arg(2)).await,
            "tab-menu-close" => {
                self.window.inner.views[0]
                    .tabs
                    .emit_by_name::<()>("setup-menu", &[&None::<adw::TabPage>]);
                self.wait_until_settled().await;
                Ok(())
            }
            "editor-menu" => match self.page() {
                Ok(page) => {
                    let view = page.view();
                    view.grab_focus();
                    match WidgetExt::activate_action(view, "menu.popup", None) {
                        Ok(()) => {
                            self.wait_until_settled().await;
                            Ok(())
                        }
                        Err(error) => Err(format!("the editor has no context menu: {error}")),
                    }
                }
                Err(error) => Err(error),
            },
            "editor-menu-close" => match self.page() {
                Ok(page) => match context_popover(page.view().upcast_ref()) {
                    Some(popover) => {
                        popover.popdown();
                        Ok(())
                    }
                    None => Err("the editor's context menu is not open".to_owned()),
                },
                Err(error) => Err(error),
            },
            "close-shortcuts" => match self.window.window().visible_dialog() {
                Some(dialog) => {
                    dialog.close();
                    Ok(())
                }
                None => Err("no dialog is open".to_owned()),
            },
            _ => return None,
        })
    }

    /// Checks an M5 assertion; `None` when `assertion` is not one.
    pub(super) fn tools_check(&self, assertion: &str, args: &[String]) -> Option<StepResult> {
        let arg = |index: usize| args[index].as_str();
        Some(match assertion {
            // The last desktop command ends with the given words (what comes before depends
            // on whether setsid and uwsm-app are installed).
            "assert-launched" => {
                let launches = self.window.shared().launches();
                match launches.last() {
                    Some(command) => {
                        let command = command
                            .iter()
                            .map(|part| part.to_string_lossy().into_owned())
                            .collect::<Vec<_>>()
                            .join(" ");
                        if command.ends_with(arg(0)) {
                            Ok(())
                        } else {
                            Err(format!("the last command was {command:?}"))
                        }
                    }
                    None => Err("nothing was launched".to_owned()),
                }
            }
            "assert-editor-key" => self.check_editor_key(arg(0), arg(1)),
            "assert-editor-keys" => self.check_editor_keys(),
            "assert-tool-stats" => {
                let stats = *self.window.inner.tools.stats.borrow();
                match (stats, boolean(arg(0)), boolean(arg(1))) {
                    (Some(stats), Ok(worker), Ok(bulk)) => {
                        println!(
                            "  # text tool: {} edits, bulk {}, worker {}, {} pieces, {:.1} ms",
                            stats.edits,
                            stats.bulk,
                            stats.worker,
                            stats.pieces,
                            stats.took.as_secs_f64() * 1e3
                        );
                        equal("(worker, bulk)", (stats.worker, stats.bulk), (worker, bulk))
                    }
                    (None, _, _) => Err("no text tool has run".to_owned()),
                    (_, Err(error), _) | (_, _, Err(error)) => Err(error),
                }
            }
            "assert-tool-pieces" => match (*self.window.inner.tools.stats.borrow(), number(arg(0)))
            {
                (Some(stats), Ok(minimum)) if stats.pieces >= minimum => Ok(()),
                (Some(stats), Ok(minimum)) => Err(format!(
                    "the result went in as {} pieces, fewer than {minimum}",
                    stats.pieces
                )),
                (None, _) => Err("no text tool has run".to_owned()),
                (_, Err(error)) => Err(error),
            },
            "assert-indentation" => match self.page() {
                Ok(page) => equal("indentation", page.indentation().label(), arg(0).to_owned()),
                Err(error) => Err(error),
            },
            "assert-map" => match (self.page(), boolean(arg(0))) {
                (Ok(page), Ok(wanted)) => equal("document map", page.map_shown(), wanted),
                (Err(error), _) | (_, Err(error)) => Err(error),
            },
            "assert-tab-menu" => match self.window.inner.views[0].tabs.menu_model() {
                Some(model) => contains_label(&model, arg(0)),
                None => Err("the tabs have no context menu".to_owned()),
            },
            "assert-editor-menu" => match self.page().map(|page| page.view().extra_menu()) {
                Ok(Some(model)) => contains_label(&model, arg(0)),
                Ok(None) => Err("the editor has no extra menu".to_owned()),
                Err(error) => Err(error),
            },
            "assert-shortcuts" => match self
                .window
                .window()
                .visible_dialog()
                .and_downcast::<adw::ShortcutsDialog>()
            {
                Some(dialog) => {
                    let text = shortcut_texts(dialog.upcast_ref()).join("\n");
                    if text.contains(arg(0)) {
                        Ok(())
                    } else {
                        Err(format!("the overview lacks {:?}", arg(0)))
                    }
                }
                None => Err("the Keyboard Shortcuts overview is not open".to_owned()),
            },
            "assert-preferences-error" => {
                let errors = self.window.shared().preferences.errors();
                let index = match arg(0) {
                    "config" => 0,
                    "keys" => 1,
                    other => return Some(Err(format!("{other}: use config or keys"))),
                };
                let found: Vec<String> = errors[index].iter().map(ToString::to_string).collect();
                match arg(1) {
                    "none" if found.is_empty() => Ok(()),
                    "none" => Err(format!("errors: {found:?}")),
                    wanted if found.iter().any(|error| error.contains(wanted)) => Ok(()),
                    wanted => Err(format!("no error has {wanted:?}; errors: {found:?}")),
                }
            }
            "assert-editor-font" => equal(
                "editor font",
                self.window
                    .shared()
                    .appearance
                    .editor_family()
                    .unwrap_or_default(),
                arg(0).to_owned(),
            ),
            "assert-line" => match (self.page(), number(arg(0))) {
                (Ok(page), Ok(line)) => {
                    let buffer = page.buffer();
                    match buffer.iter_at_line(line as i32 - 1) {
                        Some(start) => {
                            let mut end = start;
                            if !end.ends_line() {
                                end.forward_to_line_end();
                            }
                            equal(
                                &format!("line {line}"),
                                buffer.text(&start, &end, true).to_string(),
                                arg(1).to_owned(),
                            )
                        }
                        None => Err(format!("there is no line {line}")),
                    }
                }
                (Err(error), _) | (_, Err(error)) => Err(error),
            },
            "assert-dialog-selection" => match self.window.open_dialog() {
                Some(dialog) => match find_row(dialog.upcast_ref(), arg(0))
                    .and_then(|row| row.downcast::<adw::EntryRow>().ok())
                {
                    Some(entry) => equal(
                        &format!("the selection in {}", arg(0)),
                        entry
                            .selection_bounds()
                            .map(|(start, end)| entry.chars(start, end).to_string())
                            .unwrap_or_default(),
                        arg(1).to_owned(),
                    ),
                    None => Err(format!("the dialog has no entry {:?}", arg(0))),
                },
                None => Err("no dialog is open".to_owned()),
            },
            "assert-file-contains" => match std::fs::read_to_string(arg(0)) {
                Ok(text) if text.contains(arg(1)) => Ok(()),
                Ok(text) => Err(format!("{} holds {text:?}, without {:?}", arg(0), arg(1))),
                Err(error) => Err(format!("{}: {error}", arg(0))),
            },
            _ => return None,
        })
    }

    /// Waits until the current tab's text tool has finished: its worker returned and a large
    /// result is in.
    async fn wait_tool(&self) -> StepResult {
        let started = std::time::Instant::now();
        loop {
            self.wait_idle().await?;
            if !self.page()?.is_busy() && crate::worker::pending() == 0 {
                return Ok(());
            }
            if started.elapsed() > super::IDLE_TIMEOUT {
                return Err("the text tool did not finish".to_owned());
            }
            glib::timeout_future(std::time::Duration::from_millis(5)).await;
        }
    }

    /// The editors' shortcut controller of the current tab.
    fn editor_controller(&self) -> Result<gtk::ShortcutController, String> {
        let page = self.page()?;
        let controllers = page.view().observe_controllers();
        (0..controllers.n_items())
            .filter_map(|index| {
                controllers
                    .item(index)
                    .and_downcast::<gtk::ShortcutController>()
            })
            .find(|controller| controller.name().as_deref() == Some(EDITOR_SHORTCUTS))
            .ok_or_else(|| "the editor has no shortcut controller".to_owned())
    }

    /// The shortcut the editor's controller has for `key`.
    fn editor_shortcut(&self, key: &str) -> Result<gtk::Shortcut, String> {
        let controller = self.editor_controller()?;
        let wanted =
            gtk::ShortcutTrigger::parse_string(key).ok_or_else(|| format!("bad key {key}"))?;
        if controller.propagation_phase() != gtk::PropagationPhase::Capture {
            return Err("the editor's shortcuts are not in the capture phase".to_owned());
        }
        (0..controller.n_items())
            .filter_map(|index| controller.item(index).and_downcast::<gtk::Shortcut>())
            .find(|shortcut| {
                shortcut
                    .trigger()
                    .is_some_and(|trigger| trigger.equal(&wanted))
            })
            .ok_or_else(|| format!("the editor has no shortcut for {key}"))
    }

    /// What the editor does with `key`: an action's name, `swallow`, or `none`.
    fn check_editor_key(&self, key: &str, wanted: &str) -> StepResult {
        let shortcut = match self.editor_shortcut(key) {
            Err(_) if wanted == "none" => return Ok(()),
            found => found?,
        };
        let action = shortcut.action().ok_or("the shortcut has no action")?;
        let found = match action.downcast::<gtk::NamedAction>() {
            Ok(named) => named
                .action_name()
                .trim_start_matches("win.")
                .trim_start_matches("app.")
                .to_owned(),
            Err(action) if action.is::<gtk::CallbackAction>() => "swallow".to_owned(),
            Err(action) => action.to_str().to_string(),
        };
        equal(&format!("{key} in the editor"), found, wanted.to_owned())
    }

    /// Presses `key` in the current editor through its shortcut controller, as GTK does when
    /// the editor has the focus and the key reaches it.
    fn editor_key(&self, key: &str) -> StepResult {
        let page = self.page()?;
        let shortcut = self.editor_shortcut(key)?;
        let action = shortcut.action().ok_or("the shortcut has no action")?;
        if action.activate(
            gtk::ShortcutActionFlags::empty(),
            page.view().upcast_ref::<gtk::Widget>(),
            None,
        ) {
            Ok(())
        } else {
            Err(format!("{key} did nothing in the editor"))
        }
    }

    /// Every text tool's keys are the editor's shortcuts; every built-in binding ADR-009
    /// overrides is a real GtkSourceView or GtkTextView binding, and the editor takes it.
    fn check_editor_keys(&self) -> StepResult {
        let keymap = self.window.keymap();
        let mut problems = Vec::new();
        let mut count = 0;
        for id in ActionId::ALL
            .into_iter()
            .filter(|id| id.key_scope() == KeyScope::Editor)
        {
            for key in keymap.editor_keys(id) {
                count += 1;
                if let Err(error) = self.check_editor_key(&key, id.name()) {
                    problems.push(error);
                }
            }
        }
        let page = self.page()?;
        let builtins = super::class_triggers(page.view().upcast_ref());
        for key in OVERRIDDEN_BUILTINS {
            let wanted =
                gtk::ShortcutTrigger::parse_string(key).ok_or_else(|| format!("bad key {key}"))?;
            if !builtins.iter().any(|trigger| trigger.equal(&wanted)) {
                problems.push(format!(
                    "{key} is not a GtkSourceView or GtkTextView binding"
                ));
            }
            if let Err(error) = self.editor_shortcut(key) {
                problems.push(error);
            }
        }
        // AdwTabView's own shortcuts (Alt+1…9, Ctrl+Tab, Ctrl+Home…) are off: the registry
        // owns those keys (ADR-009).
        for view in &self.window.inner.views {
            let tab_shortcuts = view.tabs.shortcuts();
            if !tab_shortcuts.is_empty() {
                problems.push(format!("a tab view still has {tab_shortcuts:?}"));
            }
        }
        if problems.is_empty() {
            println!(
                "  # editor: {count} text-tool keys and {} overridden built-in bindings; \
                 the tab view's shortcuts are off",
                OVERRIDDEN_BUILTINS.len()
            );
            Ok(())
        } else {
            Err(problems.join("\n"))
        }
    }

    /// A press on tab `number`'s title or close button, or on its strip just after it, as the
    /// `presses`th press of a click. Only Stet's handling of the press runs, not the strip's:
    /// it doesn't select or close the tab.
    async fn tab_press(&self, number: &str, part: &str, presses: &str) -> StepResult {
        let index = super::number(number)?;
        let presses = i32::try_from(super::number(presses)?).map_err(|error| error.to_string())?;
        // A tab that just opened or moved slides into place: the press goes where it rests.
        let started = std::time::Instant::now();
        let mut last = None;
        while started.elapsed() < super::WAIT_TIMEOUT {
            let (bar, widget, x, y) = self.tab_part(index, part)?;
            let shown = part == "after"
                || bar
                    .pick(x, y, gtk::PickFlags::DEFAULT)
                    .is_some_and(|picked| picked == widget || picked.is_ancestor(&widget));
            if shown && last == Some((x, y)) {
                self.window.tab_pressed(&bar, presses, x, y);
                self.wait_until_settled().await;
                return Ok(());
            }
            last = Some((x, y));
            glib::timeout_future(std::time::Duration::from_millis(20)).await;
        }
        Err(format!(
            "tab {index}'s {part} never came to rest on its strip"
        ))
    }

    /// The strip of tab `index`, the widget that `part` of it is on, and where on the strip.
    fn tab_part(
        &self,
        index: usize,
        part: &str,
    ) -> Result<(gtk::Widget, gtk::Widget, f64, f64), String> {
        let (_, tab_page) = self
            .window
            .tab_at(index)
            .ok_or_else(|| format!("there is no tab {index}"))?;
        let (bar, tab) = self
            .window
            .tab_widget(&tab_page)
            .ok_or_else(|| format!("tab {index} is not on its strip"))?;
        let (width, height) = (tab.width() as f32, tab.height() as f32);
        let (widget, at) = match part {
            "title" => (tab, gtk::graphene::Point::new(width / 2.0, height / 2.0)),
            "after" => (tab, gtk::graphene::Point::new(width + 12.0, height / 2.0)),
            "close" => {
                let close = find_close_button(&tab)
                    .ok_or_else(|| format!("tab {index} shows no close button"))?;
                let at = gtk::graphene::Point::new(
                    close.width() as f32 / 2.0,
                    close.height() as f32 / 2.0,
                );
                (close, at)
            }
            other => return Err(format!("no tab part {other:?}; use title, close or after")),
        };
        let point = widget
            .compute_point(&bar, &at)
            .ok_or("the tab is not inside its strip")?;
        Ok((bar, widget, f64::from(point.x()), f64::from(point.y())))
    }

    /// Fills a field of the open dialog: an entry row's text, a combo row's choice or a
    /// switch row's state, found by its title.
    fn dialog_set(&self, title: &str, value: &str) -> StepResult {
        let dialog = self
            .window
            .open_dialog()
            .ok_or_else(|| "no dialog is open".to_owned())?;
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
}

fn indent_choice(word: &str) -> Result<IndentChoice, String> {
    Ok(match word {
        "spaces" => IndentChoice::Spaces(true),
        "tabs" => IndentChoice::Spaces(false),
        "detect" => IndentChoice::Detect,
        "to-spaces" => IndentChoice::ConvertToSpaces,
        "to-tabs" => IndentChoice::ConvertToTabs,
        other => match other.strip_prefix("width:").map(str::parse::<u32>) {
            Some(Ok(width)) => IndentChoice::Width(width),
            _ => {
                return Err(format!(
                    "no choice {other}; use spaces, tabs, width:N, detect, to-spaces or to-tabs"
                ));
            }
        },
    })
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

/// A tab's close button, while it shows.
fn find_close_button(widget: &gtk::Widget) -> Option<gtk::Widget> {
    if widget.has_css_class("tab-close-button") && widget.is_drawable() {
        return Some(widget.clone());
    }
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(found) = find_close_button(&current) {
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

/// The open context menu of a text view: GtkTextView's popover menu, a child of the view.
pub(super) fn context_popover(view: &gtk::Widget) -> Option<gtk::PopoverMenu> {
    let mut child = view.first_child();
    while let Some(current) = child {
        if let Some(popover) = current.downcast_ref::<gtk::PopoverMenu>()
            && popover.is_visible()
        {
            return Some(popover.clone());
        }
        child = current.next_sibling();
    }
    None
}

/// Whether `model`, its sections or submenus included, has an item labelled `label`.
fn contains_label(model: &gio::MenuModel, label: &str) -> StepResult {
    fn collect(model: &gio::MenuModel, out: &mut Vec<String>) {
        for index in 0..model.n_items() {
            if let Some(text) = model
                .item_attribute_value(index, "label", Some(glib::VariantTy::STRING))
                .and_then(|value| value.get::<String>())
            {
                out.push(text);
            }
            for link in ["section", "submenu"] {
                if let Some(child) = model.item_link(index, link) {
                    collect(&child, out);
                }
            }
        }
    }
    let mut labels = Vec::new();
    collect(model, &mut labels);
    if labels.iter().any(|text| text == label) {
        Ok(())
    } else {
        Err(format!("the menu has no item {label:?}; it has {labels:?}"))
    }
}

/// The titles and accelerators of a shortcuts dialog's items.
fn shortcut_texts(widget: &gtk::Widget) -> Vec<String> {
    let mut texts = labels_in(widget);
    let mut child = widget.first_child();
    while let Some(current) = child {
        if let Some(item) = current.downcast_ref::<adw::ShortcutLabel>() {
            texts.push(item.accelerator().to_string());
        }
        texts.extend(
            shortcut_texts(&current)
                .into_iter()
                .filter(|text| !text.is_empty()),
        );
        child = current.next_sibling();
    }
    texts
}

/// Points the app's home and XDG directories at `root/home`, for a script that starts with
/// `sandbox-home`: trashing, `xdg-mime` and anything else that writes there stay inside the
/// run's scratch directory. Called before GTK starts, while this is the only thread.
pub fn sandbox_home(root: &Path) -> std::io::Result<()> {
    let home = root.join("home");
    let dirs = [
        ("HOME", home.clone()),
        ("XDG_CONFIG_HOME", home.join(".config")),
        ("XDG_DATA_HOME", home.join(".local/share")),
        ("XDG_STATE_HOME", home.join(".local/state")),
        ("XDG_CACHE_HOME", home.join(".cache")),
    ];
    for (_, dir) in &dirs {
        std::fs::create_dir_all(dir)?;
    }
    for (name, dir) in dirs {
        // SAFETY: the self-test calls this before GTK or any worker starts, so no other
        // thread reads the environment.
        unsafe { std::env::set_var(name, dir) };
    }
    // SAFETY: as above.
    unsafe { std::env::remove_var("KDE_SESSION_VERSION") };
    Ok(())
}
