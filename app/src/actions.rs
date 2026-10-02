//! GActions, accelerators and the hamburger menu, all generated from the registry in
//! `stet_domain::actions`. Nothing here decides what exists; it only wires it up.

use crate::window::{Window, grouped_entries};
use gtk4 as gtk;
use gtk4::{gio, glib};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::collections::BTreeMap;
use std::path::PathBuf;
use stet_domain::actions::{
    ActionId, ActionKind, ActionScope, EDITOR_MENU, EditorMenuItem, KeyScope, Menu,
    OVERRIDDEN_BUILTINS, RECENT_SUBMENU, TAB_MENU, submenu_rank,
};
use stet_domain::settings::Keymap;

fn simple_action(id: ActionId) -> gio::SimpleAction {
    match id.kind() {
        ActionKind::Command => gio::SimpleAction::new(id.name(), None),
        ActionKind::WithString => gio::SimpleAction::new(id.name(), Some(glib::VariantTy::STRING)),
        ActionKind::Toggle => gio::SimpleAction::new_stateful(id.name(), None, &false.to_variant()),
    }
}

/// Adds every window-scoped action to `window`.
pub fn install_window_actions(window: &Window) {
    for id in ActionId::ALL
        .into_iter()
        .filter(|id| id.scope() == ActionScope::Window)
    {
        let action = simple_action(id);
        action.connect_activate(glib::clone!(
            #[weak(rename_to = inner)]
            window.inner,
            move |_, parameter| {
                let parameter = parameter.and_then(|value| value.get::<String>());
                Window { inner }.run_action(id, parameter);
            }
        ));
        window.register_action(id, action);
    }
}

/// Adds the application-scoped actions and every accelerator.
pub fn install_app_actions(app: &adw::Application) {
    for id in ActionId::ALL
        .into_iter()
        .filter(|id| id.scope() == ActionScope::App)
    {
        let action = simple_action(id);
        match id {
            // Closing the windows saves the session (M2); without one there is nothing to keep.
            ActionId::Quit => {
                action.connect_activate(glib::clone!(
                    #[weak]
                    app,
                    move |_, _| {
                        let windows = app.windows();
                        if windows.is_empty() {
                            app.quit();
                        }
                        for window in windows {
                            window.close();
                        }
                    }
                ));
            }
            other => unreachable!("{other:?} is not an application action"),
        }
        app.add_action(&action);
    }
    for id in ActionId::ALL {
        app.set_accels_for_action(&id.detailed_name(), id.default_accels());
    }
}

/// Installs the keymap's application accelerators (ADR-009, `keys.toml`).
pub fn apply_accels(app: &adw::Application, keymap: &Keymap) {
    for id in ActionId::ALL {
        let accels = keymap.accels(id);
        let accels: Vec<&str> = accels.iter().map(String::as_str).collect();
        app.set_accels_for_action(&id.detailed_name(), &accels);
    }
}

/// The editors' own shortcuts, shared by every editor's capture-phase controller: the
/// keymap's keys of the text tools ([`KeyScope::Editor`]), then the built-in GtkSourceView
/// and GtkTextView bindings ADR-009 overrides, which do nothing unless the keymap gives
/// them to an action.
pub fn fill_editor_shortcuts(store: &gio::ListStore, keymap: &Keymap) {
    let mut shortcuts = Vec::new();
    let mut bound = Vec::new();
    for id in ActionId::ALL
        .into_iter()
        .filter(|id| id.key_scope() == KeyScope::Editor)
    {
        for key in keymap.editor_keys(id) {
            let Some(trigger) = gtk::ShortcutTrigger::parse_string(&key) else {
                tracing::warn!(key, "GTK cannot parse an editor key");
                continue;
            };
            bound.push(crate::preferences::canonical(&key));
            let action = gtk::NamedAction::new(&id.detailed_name());
            shortcuts.push(gtk::Shortcut::new(Some(trigger), Some(action)));
        }
    }
    for key in OVERRIDDEN_BUILTINS {
        if bound.contains(&crate::preferences::canonical(key)) {
            continue;
        }
        if let Some(trigger) = gtk::ShortcutTrigger::parse_string(key) {
            let swallow = gtk::CallbackAction::new(|_, _| glib::Propagation::Stop);
            shortcuts.push(gtk::Shortcut::new(Some(trigger), Some(swallow)));
        }
    }
    store.splice(0, store.n_items(), &shortcuts);
}

/// The capture-phase controller an editor gets: the shared shortcuts, before GtkSourceView's
/// own bindings and only while the editor has the focus.
pub fn editor_controller(store: &gio::ListStore) -> gtk::ShortcutController {
    let controller = gtk::ShortcutController::for_model(store);
    controller.set_propagation_phase(gtk::PropagationPhase::Capture);
    controller.set_name(Some(EDITOR_SHORTCUTS));
    controller
}

/// The name of the editors' shortcut controller, for the self-test.
pub const EDITOR_SHORTCUTS: &str = "stet-editor-shortcuts";

/// Doubles underscores, which menus would otherwise read as mnemonics.
fn escape_label(label: &str) -> String {
    label.replace('_', "__")
}

/// A menu entry for `id`. GTK shows an application accelerator by itself; the text view's
/// own keys and the editors' shortcuts are shown through the `accel` attribute.
fn item(id: ActionId, keymap: &Keymap) -> gio::MenuItem {
    let item = gio::MenuItem::new(Some(id.label()), Some(&id.detailed_name()));
    if keymap.accels(id).is_empty()
        && let Some(key) = keymap.keys(id).first()
    {
        item.set_attribute_value("accel", Some(&key.to_variant()));
    }
    item
}

/// One item per encoding, grouped in submenus, each running `id` with the encoding's id.
fn character_sets(id: ActionId, target: &gio::Menu) {
    for (group, entries) in grouped_entries() {
        let submenu = gio::Menu::new();
        for entry in entries {
            submenu.append_item(&encoding_item(id, &entry.short_label(), &entry.id()));
        }
        target.append_submenu(Some(group), &submenu);
    }
}

fn encoding_item(id: ActionId, label: &str, encoding: &str) -> gio::MenuItem {
    let item = gio::MenuItem::new(Some(&escape_label(label)), None);
    item.set_action_and_target_value(Some(&id.detailed_name()), Some(&encoding.to_variant()));
    item
}

/// The status bar's line-ending menu: the registry's EOL conversions.
pub fn eol_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let section = gio::Menu::new();
    for id in [ActionId::EolCrLf, ActionId::EolLf, ActionId::EolCr] {
        section.append_item(&item(id, &Keymap::default()));
    }
    menu.append_section(Some("Save line endings as"), &section);
    menu
}

/// The submenu of the hamburger menu named `name`, by section.
fn submenu(name: &str, keymap: &Keymap) -> gio::Menu {
    let mut sections: BTreeMap<u8, gio::Menu> = BTreeMap::new();
    for id in ActionId::ALL {
        if let Some(place) = id.spec().menu.filter(|place| place.submenu == Some(name)) {
            sections
                .entry(place.section)
                .or_default()
                .append_item(&item(id, keymap));
        }
    }
    let menu = gio::Menu::new();
    for section in sections.values() {
        menu.append_section(None, section);
    }
    menu
}

/// The tab strip's context menu (`stet_domain::actions::TAB_MENU`).
pub fn tab_menu(keymap: &Keymap) -> gio::Menu {
    let menu = gio::Menu::new();
    for ids in TAB_MENU {
        let section = gio::Menu::new();
        for id in *ids {
            let item = item(*id, keymap);
            if matches!(id, ActionId::PinTab | ActionId::UnpinTab) {
                item.set_attribute_value("hidden-when", Some(&"action-disabled".to_variant()));
            }
            section.append_item(&item);
        }
        menu.append_section(None, &section);
    }
    menu
}

/// What the editor's context menu adds to GtkTextView's own (`EDITOR_MENU`).
pub fn editor_menu(keymap: &Keymap) -> gio::Menu {
    let menu = gio::Menu::new();
    for items in EDITOR_MENU {
        let section = gio::Menu::new();
        for entry in *items {
            match entry {
                EditorMenuItem::Submenu(name) => {
                    section.append_submenu(Some(name), &submenu(name, keymap));
                }
                EditorMenuItem::Action(id) => section.append_item(&item(*id, keymap)),
            }
        }
        menu.append_section(None, &section);
    }
    menu
}

/// The hamburger menu: one submenu per `Menu`, one section per registry section, the recent
/// files in File › Open Recent and every encoding in Encoding › Character Sets.
pub fn menu_model(recent: &[PathBuf], keymap: &Keymap) -> gio::Menu {
    let root = gio::Menu::new();
    for menu in Menu::ALL {
        let mut sections: BTreeMap<u8, gio::Menu> = BTreeMap::new();
        let mut nested: BTreeMap<(u8, usize, &str), BTreeMap<u8, gio::Menu>> = BTreeMap::new();
        for id in ActionId::ALL {
            let spec = id.spec();
            let Some(place) = spec.menu.filter(|place| place.menu == menu) else {
                continue;
            };
            let target = match place.submenu {
                None => sections.entry(place.section).or_default(),
                Some(submenu) => nested
                    .entry((place.parent_section, submenu_rank(submenu), submenu))
                    .or_default()
                    .entry(place.section)
                    .or_default(),
            };
            if spec.kind == ActionKind::WithString {
                if id == ActionId::OpenRecent {
                    for path in recent {
                        let Some(target_path) = path.to_str() else {
                            continue;
                        };
                        let name = path.file_name().map_or_else(
                            || target_path.to_owned(),
                            |name| name.to_string_lossy().into_owned(),
                        );
                        let item = gio::MenuItem::new(Some(&escape_label(&name)), None);
                        item.set_action_and_target_value(
                            Some(&id.detailed_name()),
                            Some(&target_path.to_variant()),
                        );
                        target.append_item(&item);
                    }
                } else if id == ActionId::Reinterpret {
                    character_sets(id, target);
                }
                continue;
            }
            target.append_item(&item(id, keymap));
        }
        for ((parent_section, _, name), submenu_sections) in nested {
            if name == RECENT_SUBMENU && recent.is_empty() {
                continue;
            }
            let submenu = gio::Menu::new();
            for section in submenu_sections
                .values()
                .filter(|section| section.n_items() > 0)
            {
                submenu.append_section(None, section);
            }
            sections
                .entry(parent_section)
                .or_default()
                .append_submenu(Some(name), &submenu);
        }
        let submenu = gio::Menu::new();
        for section in sections.values() {
            submenu.append_section(None, section);
        }
        root.append_submenu(Some(menu.label()), &submenu);
    }
    root
}

#[cfg(test)]
mod tests {
    use stet_domain::actions::ActionId;

    /// GTK's own accelerator parser, called through the C API: it needs no display, unlike
    /// the Rust wrapper, which asserts that GTK is initialised.
    fn gtk_parses(accel: &str) -> Option<(u32, u32)> {
        let text = std::ffi::CString::new(accel).ok()?;
        let (mut key, mut mods) = (0, 0);
        // SAFETY: a NUL-terminated string and two out-parameters; no display is used.
        let parsed =
            unsafe { gtk4::ffi::gtk_accelerator_parse(text.as_ptr(), &mut key, &mut mods) };
        (parsed != 0 && key != 0).then_some((key, mods))
    }

    #[test]
    fn every_registry_key_parses_as_a_gtk_accelerator() {
        for id in ActionId::ALL {
            for key in id.keys().all() {
                assert!(gtk_parses(key).is_some(), "{id:?}: GTK rejects {key}");
            }
        }
        assert!(gtk_parses("<Control>notakey").is_none());
    }

    #[test]
    fn gtk_sees_no_key_twice() {
        let mut seen = std::collections::HashMap::new();
        for id in ActionId::ALL {
            for key in id.keys().all() {
                let parsed = gtk_parses(key).unwrap();
                if let Some(other) = seen.insert(parsed, id) {
                    panic!("{key} is bound to both {other:?} and {id:?}");
                }
            }
        }
    }
}
