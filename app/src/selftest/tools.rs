//! Self-test commands for the text toolbox, the editors' shortcuts and the built-in
//! bindings ADR-009 overrides, `config.toml` and `keys.toml`, the context menus, indentation,
//! the document map, the Keyboard Shortcuts overview and Set as Default Editor
//! (docs/TESTING.md).

use super::registry::{Command, commands};
use super::script::boolean;
use super::{Harness, StepResult, equal, labels_in, number};
use crate::actions::EDITOR_SHORTCUTS;
use crate::convert::BufferPositions;
use crate::window::IndentChoice;
use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gio, glib};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::path::Path;
use stet_domain::actions::{ActionId, KeyScope, OVERRIDDEN_BUILTINS};

pub(super) const COMMANDS: &[Command] = commands![
    "sandbox-home" (0, 0) => sandbox_home,
    "set-text" (1, 1) => set_text,
    "wait-tool" (0, 0) => async wait_tool,
    "record-launches" (0, 0) => record_launches,
    "editor-key" (1, 1) => async editor_key,
    "indent-choose" (1, 1) => async indent_choose,
    "tab-menu" (1, 1) => tab_menu,
    "tab-menu-close" (0, 0) => async tab_menu_close,
    "tab-press" (3, 3) => async tab_press,
    "editor-menu" (0, 0) => async editor_menu,
    "editor-menu-close" (0, 0) => editor_menu_close,
    "close-shortcuts" (0, 0) => close_shortcuts,
    "assert-launched" (1, 1) => assert_launched,
    "assert-editor-key" (2, 2) => assert_editor_key,
    "assert-editor-keys" (0, 0) => assert_editor_keys,
    "assert-tool-stats" (2, 2) => assert_tool_stats,
    "assert-tool-pieces" (1, 1) => assert_tool_pieces,
    "assert-indentation" (1, 1) => assert_indentation,
    "assert-map" (1, 1) => assert_map,
    "assert-tab-menu" (1, 1) => assert_tab_menu,
    "assert-editor-menu" (1, 1) => assert_editor_menu,
    "assert-shortcuts" (1, 1) => assert_shortcuts,
    "assert-preferences-error" (2, 2) => assert_preferences_error,
    "assert-line" (2, 2) => assert_line,
];

impl Harness {
    /// `sandbox-home`: only as the first step; `run` points `HOME` and the XDG directories at
    /// `$TMP/home` before GTK starts, so the step itself does nothing.
    #[expect(
        clippy::unused_self,
        reason = "the registry calls every handler as a method"
    )]
    fn sandbox_home(&self, _: &[String]) {}

    /// `set-text <text>`: replaces the current tab's text without an undo step, the caret at
    /// the start.
    fn set_text(&self, args: &[String]) -> StepResult {
        let buffer = self.page()?.buffer();
        buffer.begin_irreversible_action();
        buffer.set_text(&args[0]);
        buffer.end_irreversible_action();
        buffer.place_cursor(&buffer.start_iter());
        Ok(())
    }

    /// `record-launches`: records desktop commands (terminal, file manager) instead of running
    /// them, for `assert-launched`.
    fn record_launches(&self, _: &[String]) {
        self.window.shared().record_launches();
    }

    /// `editor-key <key>`: runs the current editor's shortcut for the key (`<Control>d`) as GTK
    /// does when the key reaches the editor, and lets the main loop settle.
    async fn editor_key(&self, args: &[String]) -> StepResult {
        let pressed = self.press_editor_key(&args[0]);
        self.wait_until_settled().await;
        pressed
    }

    /// `indent-choose spaces|tabs|width:N|detect|to-spaces|to-tabs`: a choice in the
    /// indentation popover; waits for its work.
    async fn indent_choose(&self, args: &[String]) -> StepResult {
        self.window.pick_indentation(indent_choice(&args[0])?);
        self.wait_idle().await
    }

    /// `tab-menu <n>`: sets up the tab menu for tab n, as a right-click does before the menu
    /// opens.
    fn tab_menu(&self, args: &[String]) -> StepResult {
        let index = number(&args[0])?;
        let (tabs, page) = self
            .window
            .tab_at(index)
            .ok_or_else(|| format!("there is no tab {index}"))?;
        tabs.emit_by_name::<()>("setup-menu", &[&Some(page)]);
        Ok(())
    }

    /// `tab-menu-close`: what the tab menu's closing does, and lets the main loop settle.
    async fn tab_menu_close(&self, _: &[String]) {
        self.window
            .tab_view(0)
            .emit_by_name::<()>("setup-menu", &[&None::<adw::TabPage>]);
        self.wait_until_settled().await;
    }

    /// `editor-menu`: opens the current editor's context menu, with the focus in the editor.
    async fn editor_menu(&self, _: &[String]) -> StepResult {
        let page = self.page()?;
        let view = page.view();
        view.grab_focus();
        WidgetExt::activate_action(view, "menu.popup", None)
            .map_err(|error| format!("the editor has no context menu: {error}"))?;
        self.wait_until_settled().await;
        Ok(())
    }

    /// `editor-menu-close`: closes the current editor's context menu.
    fn editor_menu_close(&self, _: &[String]) -> StepResult {
        let page = self.page()?;
        let popover = context_popover(page.view().upcast_ref())
            .ok_or_else(|| "the editor's context menu is not open".to_owned())?;
        popover.popdown();
        Ok(())
    }

    /// `close-shortcuts`: closes the Keyboard Shortcuts overview (the window's dialog).
    fn close_shortcuts(&self, _: &[String]) -> StepResult {
        let dialog = self
            .window
            .window()
            .visible_dialog()
            .ok_or_else(|| "no dialog is open".to_owned())?;
        dialog.close();
        Ok(())
    }

    /// `assert-launched <words>`: the last desktop command ends with the words (what comes
    /// before depends on whether setsid and uwsm-app are installed).
    fn assert_launched(&self, args: &[String]) -> StepResult {
        let launches = self.window.shared().launches();
        let command = launches
            .last()
            .ok_or_else(|| "nothing was launched".to_owned())?
            .iter()
            .map(|part| part.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(" ");
        if command.ends_with(args[0].as_str()) {
            Ok(())
        } else {
            Err(format!("the last command was {command:?}"))
        }
    }

    /// `assert-editor-key <key> <action>|swallow|none`: what the editor does with the key: an
    /// action's name, `swallow`, or `none`.
    fn assert_editor_key(&self, args: &[String]) -> StepResult {
        self.check_editor_key(&args[0], &args[1])
    }

    /// `assert-tool-stats true|false true|false`: whether the last text tool ran on a worker,
    /// and as one bulk edit.
    fn assert_tool_stats(&self, args: &[String]) -> StepResult {
        let stats = self
            .window
            .tool_stats()
            .ok_or_else(|| "no text tool has run".to_owned())?;
        let (worker, bulk) = (boolean(&args[0])?, boolean(&args[1])?);
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

    /// `assert-tool-pieces <n>`: the last text tool's result went in as at least n pieces.
    fn assert_tool_pieces(&self, args: &[String]) -> StepResult {
        let stats = self
            .window
            .tool_stats()
            .ok_or_else(|| "no text tool has run".to_owned())?;
        let minimum = number(&args[0])?;
        if stats.pieces >= minimum {
            Ok(())
        } else {
            Err(format!(
                "the result went in as {} pieces, fewer than {minimum}",
                stats.pieces
            ))
        }
    }

    /// `assert-indentation <label>`: the current tab's indentation, as the status bar shows
    /// it.
    fn assert_indentation(&self, args: &[String]) -> StepResult {
        let page = self.page()?;
        equal("indentation", page.indentation().label(), args[0].clone())
    }

    /// `assert-map true|false`: whether the current tab shows the document map.
    fn assert_map(&self, args: &[String]) -> StepResult {
        let page = self.page()?;
        equal("document map", page.map_shown(), boolean(&args[0])?)
    }

    /// `assert-tab-menu <label>`: the tab menu has an item with that label.
    fn assert_tab_menu(&self, args: &[String]) -> StepResult {
        let model = self
            .window
            .tab_view(0)
            .menu_model()
            .ok_or_else(|| "the tabs have no context menu".to_owned())?;
        contains_label(&model, &args[0])
    }

    /// `assert-editor-menu <label>`: the editor's context menu has an item with that label.
    fn assert_editor_menu(&self, args: &[String]) -> StepResult {
        let model = self
            .page()?
            .view()
            .extra_menu()
            .ok_or_else(|| "the editor has no extra menu".to_owned())?;
        contains_label(&model, &args[0])
    }

    /// `assert-shortcuts <text>`: the open Keyboard Shortcuts overview has the text among its
    /// titles and accelerators.
    fn assert_shortcuts(&self, args: &[String]) -> StepResult {
        let dialog = self
            .window
            .window()
            .visible_dialog()
            .and_downcast::<adw::ShortcutsDialog>()
            .ok_or_else(|| "the Keyboard Shortcuts overview is not open".to_owned())?;
        let text = shortcut_texts(dialog.upcast_ref()).join("\n");
        if text.contains(args[0].as_str()) {
            Ok(())
        } else {
            Err(format!("the overview lacks {:?}", args[0]))
        }
    }

    /// `assert-preferences-error config|keys <text>|none`: an error the last load of
    /// `config.toml` or `keys.toml` reported has the text, or there is none.
    fn assert_preferences_error(&self, args: &[String]) -> StepResult {
        let errors = self.window.shared().preferences.errors();
        let index = match args[0].as_str() {
            "config" => 0,
            "keys" => 1,
            other => return Err(format!("{other}: use config or keys")),
        };
        let found: Vec<String> = errors[index].iter().map(ToString::to_string).collect();
        match args[1].as_str() {
            "none" if found.is_empty() => Ok(()),
            "none" => Err(format!("errors: {found:?}")),
            wanted if found.iter().any(|error| error.contains(wanted)) => Ok(()),
            wanted => Err(format!("no error has {wanted:?}; errors: {found:?}")),
        }
    }

    /// `assert-line <n> <text>`: line n of the current tab (1-based), without its line break.
    fn assert_line(&self, args: &[String]) -> StepResult {
        let page = self.page()?;
        let line = number(&args[0])?;
        let buffer = page.buffer();
        let start = line
            .checked_sub(1)
            .and_then(|line| buffer.iter_at_line_number(line))
            .ok_or_else(|| format!("there is no line {line}"))?;
        let mut end = start;
        if !end.ends_line() {
            end.forward_to_line_end();
        }
        equal(
            &format!("line {line}"),
            buffer.text(&start, &end, true).to_string(),
            args[1].clone(),
        )
    }

    /// `wait-tool`: waits until the current tab's text tool has finished: its worker returned
    /// and a large result is in.
    async fn wait_tool(&self, _: &[String]) -> StepResult {
        let started = std::time::Instant::now();
        loop {
            self.wait_idle().await?;
            if !self.page()?.is_busy() && crate::runtime::worker::pending() == 0 {
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
    fn press_editor_key(&self, key: &str) -> StepResult {
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

    /// `assert-editor-keys`: every text tool's keys are the editor's shortcuts; every built-in
    /// binding ADR-009 overrides is a real GtkSourceView or GtkTextView binding, and the editor
    /// takes it; the tab views' own shortcuts are off.
    fn assert_editor_keys(&self, _: &[String]) -> StepResult {
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
        for tabs in self.window.tab_views() {
            let tab_shortcuts = tabs.shortcuts();
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

    /// `tab-press <n> title|close|after <presses>`: a press on tab n's title or close button,
    /// or on its strip just after it, as that press of a click, once the tab has come to rest.
    /// Only Stet's handling of the press runs, not the strip's: it doesn't select or close the
    /// tab.
    async fn tab_press(&self, args: &[String]) -> StepResult {
        let part = args[1].as_str();
        let index = number(&args[0])?;
        let presses = i32::try_from(number(&args[2])?).map_err(|error| error.to_string())?;
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
#[expect(
    unsafe_code,
    reason = "sets the environment before any other thread starts"
)]
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
