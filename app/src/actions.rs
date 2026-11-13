//! GActions, accelerators and the menus, all generated from the registry in
//! `stet_domain::actions`. Nothing here decides what exists; it only wires it up.

use crate::window::{Window, grouped_entries};
use gtk4 as gtk;
use gtk4::{gio, glib};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use stet_domain::actions::{
    ActionId, ActionKind, ActionScope, KeyScope, MenuItem, MenuList, MenuTree, Menus,
    OVERRIDDEN_BUILTINS,
};
use stet_domain::settings::Keymap;

/// Whether `id` is there at all. In a Flatpak, Open Terminal Here (the sandbox can't start the
/// host's terminal) and Check for Updates (Flatpak updates Stet, and the command it gives is
/// pacman's) get no action, and the menus, the palette and the shortcuts leave them out
/// (INTEGRATIONS.md "Flatpak").
pub fn available(id: ActionId) -> bool {
    !(matches!(id, ActionId::OpenTerminalHere | ActionId::CheckForUpdates)
        && stet_infrastructure::sandbox::is_flatpak())
}

/// The name GTK uses to activate `id`: `app.quit`, `win.save`.
pub fn detailed_name(id: ActionId) -> String {
    let prefix = match id.scope() {
        ActionScope::App => "app",
        ActionScope::Window => "win",
    };
    format!("{prefix}.{}", id.name())
}

/// The parameter a [parameterized](ActionKind::takes_parameter) action runs with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionParam {
    /// A [`ActionKind::WithString`] action's string.
    Text(String),
    /// A [`ActionKind::WithPath`] action's path, byte for byte.
    Path(PathBuf),
}

impl ActionParam {
    /// `variant` as the parameter of an action of `kind`; `None` when the kind takes no
    /// parameter or the variant has another type.
    pub fn from_variant(kind: ActionKind, variant: &glib::Variant) -> Option<Self> {
        match kind {
            ActionKind::WithString => variant.get::<String>().map(Self::Text),
            ActionKind::WithPath => variant.get::<PathBuf>().map(Self::Path),
            ActionKind::Command | ActionKind::Toggle => None,
        }
    }
}

/// The target that runs a [`ActionKind::WithPath`] action on `path`.
pub fn path_target(path: &Path) -> glib::Variant {
    path.to_variant()
}

fn simple_action(id: ActionId) -> gio::SimpleAction {
    match id.kind() {
        ActionKind::Command => gio::SimpleAction::new(id.name(), None),
        ActionKind::WithString => gio::SimpleAction::new(id.name(), Some(glib::VariantTy::STRING)),
        ActionKind::WithPath => {
            gio::SimpleAction::new(id.name(), Some(glib::VariantTy::BYTE_STRING))
        }
        ActionKind::Toggle => gio::SimpleAction::new_stateful(id.name(), None, &false.to_variant()),
    }
}

/// Adds every window-scoped action to `window`.
pub fn install_window_actions(window: &Window) {
    for id in ActionId::ALL
        .into_iter()
        .filter(|id| id.scope() == ActionScope::Window && available(*id))
    {
        let action = simple_action(id);
        action.connect_activate(glib::clone!(
            #[weak]
            window,
            move |_, parameter| {
                let parameter =
                    parameter.and_then(|variant| ActionParam::from_variant(id.kind(), variant));
                window.run_action(id, parameter);
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
            // Closing the windows saves the session; without one there is nothing to keep.
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
        app.set_accels_for_action(&detailed_name(id), id.default_accels());
    }
}

/// Installs the keymap's application accelerators (ADR-009, `keys.toml`).
pub fn apply_accels(app: &adw::Application, keymap: &Keymap) {
    for id in ActionId::ALL {
        let accels = keymap.accels(id);
        let accels: Vec<&str> = accels.iter().map(String::as_str).collect();
        app.set_accels_for_action(&detailed_name(id), &accels);
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
            let action = gtk::NamedAction::new(&detailed_name(id));
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

/// The menus' structure, built once: what is available does not change while Stet runs.
fn menus() -> &'static Menus {
    static MENUS: LazyLock<Menus> = LazyLock::new(|| Menus::new(available));
    &MENUS
}

/// What a menu shows that the registry cannot know.
struct Shown<'a> {
    keymap: &'a Keymap,
    recent: &'a [PathBuf],
}

/// The hamburger menu, with the recent files in File › Open Recent.
pub fn menu_model(recent: &[PathBuf], keymap: &Keymap) -> gio::Menu {
    gio_menu(&menus().main, &Shown { keymap, recent })
}

/// The tab strip's context menu.
pub fn tab_menu(keymap: &Keymap) -> gio::Menu {
    gio_menu(
        &menus().tab,
        &Shown {
            keymap,
            recent: &[],
        },
    )
}

/// What the editor's context menu adds to GtkTextView's own.
pub fn editor_menu(keymap: &Keymap) -> gio::Menu {
    gio_menu(
        &menus().editor,
        &Shown {
            keymap,
            recent: &[],
        },
    )
}

/// The status bar's line-ending menu, with the default keys.
pub fn eol_menu() -> gio::Menu {
    let keymap = Keymap::default();
    gio_menu(
        &menus().line_endings,
        &Shown {
            keymap: &keymap,
            recent: &[],
        },
    )
}

/// `tree` as a GMenu: its items, then a section per non-empty section.
fn gio_menu(tree: &MenuTree, shown: &Shown) -> gio::Menu {
    let menu = gio::Menu::new();
    for item in &tree.items {
        append(&menu, item, shown);
    }
    for section in &tree.sections {
        let target = gio::Menu::new();
        for item in &section.items {
            append(&target, item, shown);
        }
        if target.n_items() > 0 {
            menu.append_section(section.label, &target);
        }
    }
    menu
}

fn append(menu: &gio::Menu, item: &MenuItem, shown: &Shown) {
    match item {
        MenuItem::Action(id) => menu.append_item(&action_item(*id, shown.keymap)),
        MenuItem::ActionWhenEnabled(id) => {
            let item = action_item(*id, shown.keymap);
            item.set_attribute_value("hidden-when", Some(&"action-disabled".to_variant()));
            menu.append_item(&item);
        }
        MenuItem::Submenu { label, menu: tree } => {
            if !tree.lists().any(|list| is_empty(list, shown)) {
                menu.append_submenu(Some(label), &gio_menu(tree, shown));
            }
        }
        MenuItem::List(id, MenuList::RecentFiles) => {
            for path in shown.recent {
                let name = path
                    .file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy();
                menu.append_item(&target_item(*id, &name, &path_target(path)));
            }
        }
        MenuItem::List(id, MenuList::Encodings) => {
            for (group, entries) in grouped_entries() {
                let submenu = gio::Menu::new();
                for entry in entries {
                    let target = entry.id().to_variant();
                    submenu.append_item(&target_item(*id, &entry.short_label(), &target));
                }
                menu.append_submenu(Some(group), &submenu);
            }
        }
    }
}

/// Whether `list` has no entries now.
fn is_empty(list: MenuList, shown: &Shown) -> bool {
    match list {
        MenuList::RecentFiles => shown.recent.is_empty(),
        MenuList::Encodings => false,
    }
}

/// A menu entry for `id`. GTK shows an application accelerator by itself; the text view's
/// own keys and the editors' shortcuts are shown through the `accel` attribute.
fn action_item(id: ActionId, keymap: &Keymap) -> gio::MenuItem {
    let item = gio::MenuItem::new(Some(id.label()), Some(&detailed_name(id)));
    if keymap.accels(id).is_empty()
        && let Some(key) = keymap.keys(id).first()
    {
        item.set_attribute_value("accel", Some(&key.to_variant()));
    }
    item
}

/// An entry running `id` with `target`, labelled `label`; underscores are doubled, as menus
/// would otherwise read them as mnemonics.
fn target_item(id: ActionId, label: &str, target: &glib::Variant) -> gio::MenuItem {
    let item = gio::MenuItem::new(Some(&label.replace('_', "__")), None);
    item.set_action_and_target_value(Some(&detailed_name(id)), Some(target));
    item
}

#[cfg(test)]
mod tests {
    use super::detailed_name;
    use gtk4::gio;
    use stet_domain::actions::{ActionId, ActionScope};

    /// `model` as indented text: each item's attributes, then its links.
    fn dump(model: &gio::MenuModel, depth: usize, out: &mut String) {
        use gtk4::gio::prelude::*;
        use std::fmt::Write as _;
        let indent = "  ".repeat(depth);
        for index in 0..model.n_items() {
            let mut attributes = Vec::new();
            let iter = model.iterate_item_attributes(index);
            while let Some((name, value)) = MenuAttributeIterExt::next(&iter) {
                attributes.push(format!("{name}={}", value.print(true)));
            }
            attributes.sort();
            let _ = writeln!(out, "{indent}- {}", attributes.join(" "));
            let mut links = Vec::new();
            let iter = model.iterate_item_links(index);
            while let Some((name, child)) = MenuLinkIterExt::next(&iter) {
                links.push((name.to_string(), child));
            }
            links.sort_by(|a, b| a.0.cmp(&b.0));
            for (name, child) in links {
                let _ = writeln!(out, "{indent}  <{name}>");
                dump(&child, depth + 2, out);
            }
        }
    }

    /// A recent file's path reaches Open Recent byte for byte, UTF-8 or not.
    #[test]
    fn path_parameters_survive_any_bytes() {
        use super::{ActionParam, path_target};
        use std::os::unix::ffi::OsStrExt as _;
        use std::path::PathBuf;
        use stet_domain::actions::ActionKind;
        for bytes in [&b"/tmp/a.md"[..], b"/tmp/bad\xff.txt", b"/"] {
            let path = PathBuf::from(std::ffi::OsStr::from_bytes(bytes));
            let target = path_target(&path);
            assert_eq!(
                ActionParam::from_variant(ActionKind::WithPath, &target),
                Some(ActionParam::Path(path))
            );
            assert_eq!(
                ActionParam::from_variant(ActionKind::WithString, &target),
                None
            );
        }
    }

    /// The GMenus the window shows: labels, actions, targets, attributes and links. GIO
    /// builds them without a display.
    #[test]
    fn the_menus_keep_their_models() {
        use gtk4::prelude::*;
        use std::fmt::Write as _;
        use std::os::unix::ffi::OsStrExt as _;
        use std::path::PathBuf;
        let keymap = stet_domain::settings::Keymap::default();
        let recent = [
            PathBuf::from("/home/user/notes/todo_list.txt"),
            PathBuf::from("/tmp/a.md"),
            PathBuf::from(std::ffi::OsStr::from_bytes(b"/tmp/bad\xff.txt")),
            PathBuf::from("/"),
        ];
        let mut out = String::new();
        for (name, model) in [
            (
                "main with recent files",
                super::menu_model(&recent, &keymap),
            ),
            ("main without recent files", super::menu_model(&[], &keymap)),
            ("tab", super::tab_menu(&keymap)),
            ("editor", super::editor_menu(&keymap)),
            ("line endings", super::eol_menu()),
        ] {
            writeln!(out, "# {name}").unwrap();
            dump(model.upcast_ref(), 0, &mut out);
        }
        insta::assert_snapshot!(out);
    }

    #[test]
    fn detailed_names_carry_the_scope_and_resolve() {
        assert_eq!(detailed_name(ActionId::Quit), "app.quit");
        assert_eq!(detailed_name(ActionId::NewTab), "win.new-tab");
        for id in ActionId::ALL {
            let name = detailed_name(id);
            let prefix = match id.scope() {
                ActionScope::App => "app.",
                ActionScope::Window => "win.",
            };
            assert!(name.starts_with(prefix), "{name}");
            assert_eq!(ActionId::from_name(&name), Some(id));
        }
    }

    /// GTK's own accelerator parser, called through the C API: it needs no display, unlike
    /// the Rust wrapper, which asserts that GTK is initialised.
    #[expect(unsafe_code, reason = "GTK's parser without a display")]
    fn gtk_parses(accel: &str) -> Option<(u32, u32)> {
        let text = std::ffi::CString::new(accel).ok()?;
        let (mut key, mut mods) = (0, 0);
        // SAFETY: a NUL-terminated string and two out-parameters; no display is used.
        let parsed =
            unsafe { gtk4::ffi::gtk_accelerator_parse(text.as_ptr(), &raw mut key, &raw mut mods) };
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
