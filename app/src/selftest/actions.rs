//! Self-test commands for the action registry: running actions by name and from the command
//! palette, their state, and that the accelerators, the menu and the editor's keys follow
//! the registry.

use super::registry::{Command, commands};
use super::script::boolean;
use super::{Harness, StepResult, action_id, class_triggers, equal};
use crate::actions::detailed_name;
use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gio, glib};
use stet_domain::actions::{ActionId, ActionScope};
use stet_domain::document::language::hint_ids;

pub(super) const COMMANDS: &[Command] = commands![
    "action" (1, 2) => async action,
    "action-nowait" (1, 1) => action_nowait,
    "palette" (0, 1) => palette,
    "palette-activate" (0, 0) => async palette_activate,
    "palette-close" (0, 0) => palette_close,
    "assert-action-state" (2, 2) => assert_action_state,
    "assert-action-enabled" (2, 2) => assert_action_enabled,
    "assert-accels" (0, 0) => assert_accels,
    "assert-menu" (0, 0) => assert_menu,
    "assert-menu-contains" (1, 1) => assert_menu_contains,
    "assert-palette-first" (1, 1) => assert_palette_first,
    "assert-palette-contains" (1, 1) => assert_palette_contains,
    "assert-language-hints" (0, 0) => assert_language_hints,
    "assert-widget-keys" (0, 0) => assert_widget_keys,
];

impl Harness {
    /// `action <name> [<string parameter>]`: activates a registry action through its GAction
    /// and lets the main loop settle.
    async fn action(&self, args: &[String]) -> StepResult {
        self.activate(&args[0], args.get(1).map(String::as_str))?;
        self.wait_until_settled().await;
        Ok(())
    }

    /// `action-nowait <name>`: activates an action and goes on at once, so the next step runs
    /// while its worker does.
    fn action_nowait(&self, args: &[String]) -> StepResult {
        self.activate(&args[0], None)
    }

    /// `palette [<query>]`: opens the command palette with the query.
    fn palette(&self, args: &[String]) {
        self.window
            .open_palette(args.first().map_or("", String::as_str));
    }

    /// `palette-activate`: runs the palette's selected row and waits for its work.
    async fn palette_activate(&self, _: &[String]) -> StepResult {
        self.window.palette_activate();
        self.wait_idle().await
    }

    /// `palette-close`: closes the command palette.
    fn palette_close(&self, _: &[String]) {
        self.window.close_palette();
    }

    /// `assert-action-state <name> true|false`: a window action's boolean state.
    fn assert_action_state(&self, args: &[String]) -> StepResult {
        let id = action_id(&args[0])?;
        let state = self
            .window
            .action(id)
            .and_then(|action| action.state())
            .and_then(|state| state.get::<bool>())
            .ok_or_else(|| format!("{} has no boolean state", args[0]))?;
        equal("state", state, boolean(&args[1])?)
    }

    /// `assert-action-enabled <name> true|false`: whether a window action is enabled.
    fn assert_action_enabled(&self, args: &[String]) -> StepResult {
        let id = action_id(&args[0])?;
        let enabled = self
            .window
            .action(id)
            .is_some_and(|action| action.is_enabled());
        equal("enabled", enabled, boolean(&args[1])?)
    }

    /// `assert-accels`: every accelerator of the keymap (the registry's, with `keys.toml`) is
    /// installed and parses in GTK.
    fn assert_accels(&self, _: &[String]) -> StepResult {
        let keymap = self.window.keymap();
        let mut problems = Vec::new();
        for id in ActionId::ALL {
            let installed: Vec<_> = self
                .app
                .accels_for_action(&detailed_name(id))
                .iter()
                .filter_map(|accel| gtk::accelerator_parse(accel.as_str()))
                .collect();
            let accels = keymap.accels(id);
            let wanted: Vec<_> = accels
                .iter()
                .map(|accel| gtk::accelerator_parse(accel).ok_or(accel.as_str()))
                .collect();
            for accel in &wanted {
                match accel {
                    Err(text) => problems.push(format!("{id:?}: GTK cannot parse {text}")),
                    Ok(parsed) if !installed.contains(parsed) => {
                        problems.push(format!("{id:?}: {parsed:?} is not installed"));
                    }
                    Ok(_) => {}
                }
            }
            if installed.len() != wanted.len() {
                problems.push(format!(
                    "{id:?}: {} installed, {} wanted",
                    installed.len(),
                    wanted.len()
                ));
            }
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(problems.join("\n"))
        }
    }

    /// Every (label, action) in the hamburger menu, submenus included.
    fn menu_items(&self) -> Result<Vec<(String, String)>, String> {
        fn collect(model: &gio::MenuModel, out: &mut Vec<(String, String)>) {
            let text = |index: i32, attribute: &str| {
                model
                    .item_attribute_value(index, attribute, Some(glib::VariantTy::STRING))
                    .and_then(|value| value.get::<String>())
                    .unwrap_or_default()
            };
            for index in 0..model.n_items() {
                let action = text(index, "action");
                if !action.is_empty() {
                    out.push((text(index, "label"), action));
                }
                for link in ["section", "submenu"] {
                    if let Some(child) = model.item_link(index, link) {
                        collect(&child, out);
                    }
                }
            }
        }
        let model = self
            .window
            .menu_model()
            .ok_or_else(|| "the window has no menu".to_owned())?;
        let mut items = Vec::new();
        collect(&model, &mut items);
        Ok(items)
    }

    /// `assert-menu`: the menu holds every registry action with a menu place, and nothing
    /// else.
    fn assert_menu(&self, _: &[String]) -> StepResult {
        let items = self.menu_items()?;
        let mut problems = self.missing_from_menu(&items);
        for (label, action) in &items {
            let installed = ActionId::from_name(action).is_some_and(|id| match id.scope() {
                ActionScope::App => self.app.lookup_action(id.name()).is_some(),
                ActionScope::Window => self.window.action(id).is_some(),
            });
            if !installed {
                problems.push(format!(
                    "menu item {label:?} runs {action}, which is not installed"
                ));
            }
        }
        if problems.is_empty() {
            println!("  # menu: {} items, all from the registry", items.len());
            Ok(())
        } else {
            Err(problems.join("\n"))
        }
    }

    /// The registry actions with a menu place that `items` lacks.
    fn missing_from_menu(&self, items: &[(String, String)]) -> Vec<String> {
        let no_recent = self.window.shared().recent().is_empty();
        let recent_submenu = ActionId::OpenRecent
            .spec()
            .menu
            .and_then(|place| place.submenu);
        let mut problems = Vec::new();
        for id in ActionId::ALL {
            let spec = id.spec();
            let Some(place) = spec.menu else {
                continue;
            };
            let hidden_recent = no_recent && place.submenu == recent_submenu;
            if hidden_recent || spec.kind.takes_parameter() {
                continue;
            }
            let found = items
                .iter()
                .any(|(label, action)| label == spec.label && *action == detailed_name(id));
            if !found {
                problems.push(format!("{id:?} ({}) is not in the menu", spec.label));
            }
        }
        problems
    }

    /// `assert-menu-contains <label>`: the menu, submenus included, has an item with that
    /// label.
    fn assert_menu_contains(&self, args: &[String]) -> StepResult {
        let items = self.menu_items()?;
        if items.iter().any(|(label, _)| *label == args[0]) {
            Ok(())
        } else {
            Err(format!("the menu has no item {:?}", args[0]))
        }
    }

    /// `assert-palette-first <label>`: the palette's first row.
    fn assert_palette_first(&self, args: &[String]) -> StepResult {
        let labels = self.window.palette_labels();
        equal(
            "first palette row",
            labels.first().cloned().unwrap_or_default(),
            args[0].clone(),
        )
    }

    /// `assert-palette-contains <label>`: the palette has a row with that label.
    fn assert_palette_contains(&self, args: &[String]) -> StepResult {
        let labels = self.window.palette_labels();
        if labels.iter().any(|label| *label == args[0]) {
            Ok(())
        } else {
            Err(format!("palette rows {labels:?} lack {:?}", args[0]))
        }
    }

    /// `assert-language-hints`: every language id the registry hints at is installed.
    #[expect(
        clippy::unused_self,
        reason = "the registry calls every handler as a method"
    )]
    fn assert_language_hints(&self, _: &[String]) -> StepResult {
        let manager = sourceview5::LanguageManager::default();
        let missing: Vec<&str> = hint_ids()
            .filter(|id| manager.language(id).is_none())
            .collect();
        if missing.is_empty() {
            Ok(())
        } else {
            Err(format!("not installed: {missing:?}"))
        }
    }

    /// `assert-widget-keys`: every widget key the registry shows for editing actions is bound
    /// by GtkTextView.
    fn assert_widget_keys(&self, _: &[String]) -> StepResult {
        let page = self.page()?;
        let triggers = class_triggers(page.view().upcast_ref());
        let mut missing = Vec::new();
        for id in ActionId::ALL {
            for key in id.keys().widget {
                let wanted = gtk::ShortcutTrigger::parse_string(key)
                    .ok_or_else(|| format!("GTK cannot parse {key}"))?;
                if !triggers.iter().any(|trigger| trigger.equal(&wanted)) {
                    missing.push(format!("{id:?}: {key}"));
                }
            }
        }
        if missing.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "not bound by the text view: {}",
                missing.join(", ")
            ))
        }
    }
}
