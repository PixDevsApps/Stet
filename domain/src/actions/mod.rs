//! The action registry: every user-visible command with its GAction name, scope, label, menu
//! place, default keys (ADR-009) and palette visibility. The hamburger menu, the command palette
//! and the accelerators are all generated from it, so a menu entry cannot exist without a
//! working action.

mod accelerator;
mod menu;
mod registry;

pub use accelerator::{Accelerator, OVERRIDDEN_BUILTINS, is_key_name};
use menu::{COMPARE_SUBMENU, LINES_SUBMENU};
pub use menu::{Menu, MenuItem, MenuList, MenuPlace, MenuSection, MenuTree, Menus};
pub use registry::ActionId;

/// Where an action lives: on the application, or on the window that has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionScope {
    /// On the application (`app.`), working with or without a window.
    App,
    /// On the window that has focus (`win.`).
    Window,
}

/// Where an action's keys work (ADR-009 amendment on text-tool keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyScope {
    /// Application accelerators: anywhere in the window, before the focused widget.
    Window,
    /// Only while an editor has the keyboard focus, before GtkSourceView's own bindings. The
    /// text tools use it, so their keys never change the text from the find bar or a list.
    Editor,
}

/// The shape of the GAction behind an [`ActionId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    /// A plain command.
    Command,
    /// A command with a string parameter (GVariant type `s`). Its menu entries, if any, are
    /// generated at run time, one per value (for example one per encoding).
    WithString,
    /// A command with a file path parameter, carried as bytes (GVariant type `ay`) so a name
    /// that isn't UTF-8 survives. Its menu entries are generated at run time, one per path.
    WithPath,
    /// An on/off setting with a boolean state, shown with a check mark.
    Toggle,
}

impl ActionKind {
    /// Whether the action runs with a parameter, given by its own menu entries or pickers; such
    /// an action has no keys and no plain palette entry.
    #[must_use]
    pub const fn takes_parameter(self) -> bool {
        matches!(self, Self::WithString | Self::WithPath)
    }
}

/// Default keys, as GTK accelerator strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Keys {
    /// Installed by Stet, where the action's [`KeyScope`] says: as application accelerators,
    /// which GTK handles in the capture phase before the focused widget sees the key, or as
    /// the editors' own shortcuts.
    pub app: &'static [&'static str],
    /// Handled by the focused text widget itself (GtkTextView, or the find entry's GtkText).
    /// They are shown in menus and the palette but never installed as accelerators, which
    /// would steal them from text entries.
    pub widget: &'static [&'static str],
}

impl Keys {
    const NONE: Self = Self::app(&[]);

    const fn app(app: &'static [&'static str]) -> Self {
        Self { app, widget: &[] }
    }

    const fn widget(widget: &'static [&'static str]) -> Self {
        Self { app: &[], widget }
    }

    /// Every key, application accelerators first. The first one is the one menus show.
    pub fn all(self) -> impl Iterator<Item = &'static str> {
        self.app.iter().chain(self.widget).copied()
    }

    /// Whether there are no keys at all.
    #[cfg(test)]
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.app.is_empty() && self.widget.is_empty()
    }
}

/// Everything the registry knows about one action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionSpec {
    /// The GAction name, stable across releases (keymaps and scripts refer to it).
    pub name: &'static str,
    /// Whether the action is the application's or the window's.
    pub scope: ActionScope,
    /// The shape of the GAction.
    pub kind: ActionKind,
    /// The label menus and the palette show.
    pub label: &'static str,
    /// Where the hamburger menu lists the action, if it does.
    pub menu: Option<MenuPlace>,
    /// The default keys.
    pub keys: Keys,
    /// Where the keys work.
    pub key_scope: KeyScope,
    /// Listed in the command palette. Parameterized actions are reached through their own
    /// entries instead (recent files, the language picker).
    pub palette: bool,
}

const fn command(
    name: &'static str,
    label: &'static str,
    menu: Option<MenuPlace>,
    keys: Keys,
) -> ActionSpec {
    ActionSpec {
        name,
        scope: ActionScope::Window,
        kind: ActionKind::Command,
        label,
        menu,
        keys,
        key_scope: KeyScope::Window,
        palette: true,
    }
}

/// A text tool: a command whose keys work only in the editor ([`KeyScope::Editor`]).
const fn editing(
    name: &'static str,
    label: &'static str,
    menu: Option<MenuPlace>,
    keys: Keys,
) -> ActionSpec {
    ActionSpec {
        key_scope: KeyScope::Editor,
        ..command(name, label, menu, keys)
    }
}

const fn toggle(name: &'static str, label: &'static str, menu: Option<MenuPlace>) -> ActionSpec {
    ActionSpec {
        kind: ActionKind::Toggle,
        ..command(name, label, menu, Keys::NONE)
    }
}

/// One of Line Operations' sorts; none has a key.
const fn sort(name: &'static str, label: &'static str) -> ActionSpec {
    editing(
        name,
        label,
        MenuPlace::nested_in(Menu::Edit, 3, LINES_SUBMENU, 3),
        Keys::NONE,
    )
}

/// A command in one of the Search menu's marking submenus.
const fn in_search(
    name: &'static str,
    label: &'static str,
    submenu: &'static str,
    section: u8,
    keys: Keys,
) -> ActionSpec {
    command(
        name,
        label,
        MenuPlace::nested_in(Menu::Search, 3, submenu, section),
        keys,
    )
}

/// A Jump Up or Jump Down command. It moves the editor's selection, so its keys work in
/// the editor, as the text tools' do, and never take a dialog's mnemonic (Alt+M).
const fn jump(
    name: &'static str,
    label: &'static str,
    submenu: &'static str,
    section: u8,
    keys: Keys,
) -> ActionSpec {
    ActionSpec {
        key_scope: KeyScope::Editor,
        ..in_search(name, label, submenu, section, keys)
    }
}

/// A command in Tools › Compare.
const fn in_compare(
    name: &'static str,
    label: &'static str,
    section: u8,
    keys: Keys,
) -> ActionSpec {
    command(
        name,
        label,
        MenuPlace::nested(Menu::Tools, COMPARE_SUBMENU, section),
        keys,
    )
}

/// A command that takes the id of an encoding (`stet_infrastructure::encoding`'s
/// `EncodingEntry::id`) from its menu entries.
const fn with_encoding(
    name: &'static str,
    label: &'static str,
    menu: Option<MenuPlace>,
) -> ActionSpec {
    ActionSpec {
        kind: ActionKind::WithString,
        palette: false,
        ..command(name, label, menu, Keys::NONE)
    }
}
