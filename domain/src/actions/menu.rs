//! The menus built from the registry: the hamburger menu's places and nested submenus, the tab
//! strip's and the editor's context menus and the status bar's line-ending menu. [`Menus`] holds
//! their structure and every ordering rule; the app only turns it into GTK's menu models.

use super::ActionId;
use std::collections::BTreeMap;

/// The hamburger menu's submenus, in the order the menu lists them, and its first page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Menu {
    File,
    Edit,
    Search,
    View,
    Encoding,
    Language,
    Settings,
    Tools,
    Help,
    /// The menu's first page itself, below its submenus: not one of [`Menu::ALL`].
    Main,
}

impl Menu {
    /// Every place in the menu, in its order: the submenus, then the first page's own items.
    pub const PLACES: [Self; 10] = [
        Self::File,
        Self::Edit,
        Self::Search,
        Self::View,
        Self::Encoding,
        Self::Language,
        Self::Settings,
        Self::Tools,
        Self::Help,
        Self::Main,
    ];

    /// The submenus, in the menu's order.
    pub const ALL: [Self; 9] = [
        Self::File,
        Self::Edit,
        Self::Search,
        Self::View,
        Self::Encoding,
        Self::Language,
        Self::Settings,
        Self::Tools,
        Self::Help,
    ];

    /// The name the menu shows.
    pub const fn label(self) -> &'static str {
        match self {
            Self::File => "File",
            Self::Edit => "Edit",
            Self::Search => "Search",
            Self::View => "View",
            Self::Encoding => "Encoding",
            Self::Language => "Language",
            Self::Settings => "Settings",
            Self::Tools => "Tools",
            Self::Help => "Help",
            Self::Main => "Menu",
        }
    }
}

/// A menu entry's position: a submenu, an optional nested submenu, and a section inside it.
/// Sections are drawn with separators between them. A nested submenu itself sits in the
/// parent's section `parent_section`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuPlace {
    pub menu: Menu,
    pub submenu: Option<&'static str>,
    pub section: u8,
    pub parent_section: u8,
}

impl MenuPlace {
    #[expect(
        clippy::unnecessary_wraps,
        reason = "constructors for the `Option<MenuPlace>` column of the action table"
    )]
    pub(super) const fn at(menu: Menu, section: u8) -> Option<Self> {
        Some(Self {
            menu,
            submenu: None,
            section,
            parent_section: 0,
        })
    }

    pub(super) const fn nested(menu: Menu, submenu: &'static str, section: u8) -> Option<Self> {
        Self::nested_in(menu, 0, submenu, section)
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "constructors for the `Option<MenuPlace>` column of the action table"
    )]
    pub(super) const fn nested_in(
        menu: Menu,
        parent_section: u8,
        submenu: &'static str,
        section: u8,
    ) -> Option<Self> {
        Some(Self {
            menu,
            submenu: Some(submenu),
            section,
            parent_section,
        })
    }
}

/// The submenu that holds the recent files.
pub(super) const RECENT_SUBMENU: &str = "Open Recent";

/// Edit › EOL Conversion.
pub(super) const EOL_SUBMENU: &str = "EOL Conversion";

/// Encoding › Character Sets: every encoding, grouped, to reinterpret the file as.
pub(super) const CHARACTER_SETS_SUBMENU: &str = "Character Sets";

/// Edit › Copy to Clipboard: the document's path and name.
pub(super) const COPY_SUBMENU: &str = "Copy to Clipboard";

/// Edit › Convert Case to.
pub(super) const CASE_SUBMENU: &str = "Convert Case to";

/// Edit › Line Operations.
pub(super) const LINES_SUBMENU: &str = "Line Operations";

/// Edit › Comment/Uncomment.
pub(super) const COMMENT_SUBMENU: &str = "Comment/Uncomment";

/// Edit › Blank Operations.
pub(super) const BLANK_SUBMENU: &str = "Blank Operations";

/// Tools › JSON and Tools › XML.
pub(super) const JSON_SUBMENU: &str = "JSON";
pub(super) const XML_SUBMENU: &str = "XML";

/// Tools › Compare: the comparison commands.
pub(super) const COMPARE_SUBMENU: &str = "Compare";

/// Search › Style All Occurrences of Token, Clear Style, Jump Up, Jump Down and Bookmark.
pub(super) const STYLE_TOKEN_SUBMENU: &str = "Style All Occurrences of Token";
pub(super) const CLEAR_STYLE_SUBMENU: &str = "Clear Style";
pub(super) const JUMP_UP_SUBMENU: &str = "Jump Up";
pub(super) const JUMP_DOWN_SUBMENU: &str = "Jump Down";
pub(super) const BOOKMARK_SUBMENU: &str = "Bookmark";

/// View › Move/Clone Current Document.
pub(super) const MOVE_CLONE_SUBMENU: &str = "Move/Clone Current Document";

/// Nested submenus in the order a section lists them, after the section's own actions.
const SUBMENU_ORDER: &[&str] = &[
    RECENT_SUBMENU,
    COPY_SUBMENU,
    CASE_SUBMENU,
    LINES_SUBMENU,
    COMMENT_SUBMENU,
    EOL_SUBMENU,
    BLANK_SUBMENU,
    CHARACTER_SETS_SUBMENU,
    STYLE_TOKEN_SUBMENU,
    CLEAR_STYLE_SUBMENU,
    JUMP_UP_SUBMENU,
    JUMP_DOWN_SUBMENU,
    BOOKMARK_SUBMENU,
    MOVE_CLONE_SUBMENU,
    COMPARE_SUBMENU,
    JSON_SUBMENU,
    XML_SUBMENU,
];

/// Where `submenu` goes among the nested submenus of a section.
fn submenu_rank(submenu: &str) -> usize {
    SUBMENU_ORDER
        .iter()
        .position(|name| *name == submenu)
        .unwrap_or(SUBMENU_ORDER.len())
}

/// A menu: its own items, then its sections, which are drawn with separators between them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MenuTree {
    pub items: Vec<MenuItem>,
    pub sections: Vec<MenuSection>,
}

impl MenuTree {
    /// The run-time lists among the menu's own items and its sections' items, not its
    /// submenus'.
    pub fn lists(&self) -> impl Iterator<Item = MenuList> {
        self.items
            .iter()
            .chain(self.sections.iter().flat_map(|section| &section.items))
            .filter_map(|item| match item {
                MenuItem::List(_, list) => Some(*list),
                _ => None,
            })
    }

    fn of_sections(sections: Vec<MenuSection>) -> Self {
        Self {
            items: Vec::new(),
            sections,
        }
    }
}

/// A group of items, with an optional heading.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MenuSection {
    pub label: Option<&'static str>,
    pub items: Vec<MenuItem>,
}

/// A menu entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuItem {
    /// Runs the action, labelled with its registry label.
    Action(ActionId),
    /// Runs the action and is hidden while it is disabled: of two alternatives, only the one
    /// that applies shows.
    ActionWhenEnabled(ActionId),
    Submenu {
        label: &'static str,
        menu: MenuTree,
    },
    /// One entry per value of a list known only at run time, each running the action (one
    /// that [takes a parameter](super::ActionKind::takes_parameter)) with its value. A submenu
    /// that holds a list is left out while the list is empty, and a section the list leaves
    /// empty is not drawn.
    List(ActionId, MenuList),
}

/// The run-time lists the menus show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuList {
    /// The recent files, most recent first.
    RecentFiles,
    /// Every encoding, grouped in submenus.
    Encodings,
}

/// Every menu Stet shows, built from the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menus {
    /// The hamburger menu: one submenu per [`Menu::ALL`], then the sections of [`Menu::Main`].
    pub main: MenuTree,
    /// The tab strip's context menu; each action acts on the tab that was clicked.
    pub tab: MenuTree,
    /// What the editor's context menu adds after GtkTextView's own clipboard items.
    pub editor: MenuTree,
    /// The status bar's line-ending menu.
    pub line_endings: MenuTree,
}

impl Menus {
    /// The menus of the actions `available` accepts; the others are left out everywhere.
    pub fn new(available: impl Fn(ActionId) -> bool) -> Self {
        let available = &available;
        Self {
            main: main_menu(available),
            tab: fixed_menu(TAB_MENU, available),
            editor: fixed_menu(EDITOR_MENU, available),
            line_endings: MenuTree::of_sections(
                fixed_menu(LINE_ENDING_MENU, available)
                    .sections
                    .into_iter()
                    .map(|section| MenuSection {
                        label: Some("Save line endings as"),
                        ..section
                    })
                    .collect(),
            ),
        }
    }
}

/// An entry of the fixed menus, which are tables rather than registry places.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fixed {
    Action(ActionId),
    /// See [`MenuItem::ActionWhenEnabled`].
    WhenEnabled(ActionId),
    /// Every action of a nested submenu, as the hamburger menu has it.
    Submenu(&'static str),
}

/// The tab strip's context menu, by section.
const TAB_MENU: &[&[Fixed]] = &[
    &[
        Fixed::WhenEnabled(ActionId::PinTab),
        Fixed::WhenEnabled(ActionId::UnpinTab),
    ],
    &[
        Fixed::Action(ActionId::CloseTab),
        Fixed::Action(ActionId::CloseOthers),
        Fixed::Action(ActionId::CloseToTheRight),
    ],
    &[
        Fixed::Action(ActionId::Save),
        Fixed::Action(ActionId::SaveAs),
    ],
    &[
        Fixed::Action(ActionId::CopyFullPath),
        Fixed::Action(ActionId::CopyFileName),
        Fixed::Action(ActionId::CopyDirectoryPath),
    ],
    &[
        Fixed::Action(ActionId::OpenContainingFolder),
        Fixed::Action(ActionId::OpenTerminalHere),
    ],
    &[
        Fixed::Action(ActionId::RenameFile),
        Fixed::Action(ActionId::MoveToTrash),
    ],
];

/// The editor's context menu, by section.
const EDITOR_MENU: &[&[Fixed]] = &[
    &[
        Fixed::Submenu(CASE_SUBMENU),
        Fixed::Submenu(COMMENT_SUBMENU),
        Fixed::Submenu(LINES_SUBMENU),
    ],
    &[
        Fixed::Action(ActionId::SelectAndFindNext),
        Fixed::Action(ActionId::GoToMatchingBrace),
    ],
];

/// The status bar's line-ending menu: the EOL conversions.
const LINE_ENDING_MENU: &[&[Fixed]] = &[&[
    Fixed::Action(ActionId::EolCrLf),
    Fixed::Action(ActionId::EolLf),
    Fixed::Action(ActionId::EolCr),
]];

/// A fixed menu, section by section, without the unavailable actions and the sections they
/// leave empty.
fn fixed_menu(table: &[&[Fixed]], available: &dyn Fn(ActionId) -> bool) -> MenuTree {
    let sections = table.iter().map(|entries| MenuSection {
        label: None,
        items: entries
            .iter()
            .filter_map(|entry| match *entry {
                Fixed::Action(id) => available(id).then_some(MenuItem::Action(id)),
                Fixed::WhenEnabled(id) => available(id).then_some(MenuItem::ActionWhenEnabled(id)),
                Fixed::Submenu(name) => Some(MenuItem::Submenu {
                    label: name,
                    menu: nested_menu(name, available),
                }),
            })
            .collect(),
    });
    MenuTree::of_sections(
        sections
            .filter(|section| !section.items.is_empty())
            .collect(),
    )
}

/// The hamburger menu.
fn main_menu(available: &dyn Fn(ActionId) -> bool) -> MenuTree {
    MenuTree {
        items: Menu::ALL
            .into_iter()
            .map(|menu| MenuItem::Submenu {
                label: menu.label(),
                menu: place_menu(menu, available),
            })
            .collect(),
        sections: place_menu(Menu::Main, available).sections,
    }
}

/// The available actions with a place in the hamburger menu, in the registry's order.
fn placed(available: &dyn Fn(ActionId) -> bool) -> impl Iterator<Item = (ActionId, MenuPlace)> {
    ActionId::ALL
        .into_iter()
        .filter(move |id| available(*id))
        .filter_map(|id| Some((id, id.spec().menu?)))
}

/// What the menus show for `id`: the action, or the run-time list a parameterized action
/// runs with.
fn entry(id: ActionId) -> Option<MenuItem> {
    if !id.kind().takes_parameter() {
        return Some(MenuItem::Action(id));
    }
    match id {
        ActionId::OpenRecent => Some(MenuItem::List(id, MenuList::RecentFiles)),
        ActionId::Reinterpret => Some(MenuItem::List(id, MenuList::Encodings)),
        _ => None,
    }
}

/// `entries` grouped by section number, in the numbers' order; each section keeps its
/// entries' order.
fn sections(entries: impl IntoIterator<Item = (u8, MenuItem)>) -> Vec<MenuSection> {
    let mut sections: BTreeMap<u8, Vec<MenuItem>> = BTreeMap::new();
    for (section, item) in entries {
        sections.entry(section).or_default().push(item);
    }
    sections
        .into_values()
        .map(|items| MenuSection { label: None, items })
        .collect()
}

/// One of the hamburger menu's places: its own actions in the registry's order, then its
/// nested submenus in [`SUBMENU_ORDER`], each in the section its place names.
fn place_menu(menu: Menu, available: &dyn Fn(ActionId) -> bool) -> MenuTree {
    let here: Vec<(ActionId, MenuPlace)> = placed(available)
        .filter(|(_, place)| place.menu == menu)
        .collect();
    let mut nested: Vec<(u8, &'static str)> = here
        .iter()
        .filter_map(|(_, place)| Some((place.parent_section, place.submenu?)))
        .collect();
    nested.sort_by_key(|&(section, name)| (section, submenu_rank(name), name));
    nested.dedup();
    let own = here
        .iter()
        .filter(|(_, place)| place.submenu.is_none())
        .filter_map(|(id, place)| Some((place.section, entry(*id)?)));
    let submenus = nested.into_iter().map(|(section, name)| {
        let item = MenuItem::Submenu {
            label: name,
            menu: nested_menu(name, available),
        };
        (section, item)
    });
    MenuTree::of_sections(sections(own.chain(submenus)))
}

/// The nested submenu `name`, by section.
fn nested_menu(name: &str, available: &dyn Fn(ActionId) -> bool) -> MenuTree {
    MenuTree::of_sections(sections(
        placed(available)
            .filter(|(_, place)| place.submenu == Some(name))
            .filter_map(|(id, place)| Some((place.section, entry(id)?))),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{ActionKind, ActionScope, KeyScope};
    use std::fmt::Write as _;

    /// `tree` as indented text: one line per item, `--` between sections.
    fn dump(tree: &MenuTree, depth: usize, out: &mut String) {
        let indent = "  ".repeat(depth);
        let item = |item: &MenuItem, out: &mut String| match item {
            MenuItem::Action(id) => {
                let _ = writeln!(out, "{indent}{} [{}]", id.label(), id.name());
            }
            MenuItem::ActionWhenEnabled(id) => {
                let _ = writeln!(out, "{indent}{} [{}, when enabled]", id.label(), id.name());
            }
            MenuItem::Submenu { label, menu } => {
                let _ = writeln!(out, "{indent}{label} ›");
                dump(menu, depth + 1, out);
            }
            MenuItem::List(id, list) => {
                let _ = writeln!(out, "{indent}<{list:?}> [{}]", id.name());
            }
        };
        for entry in &tree.items {
            item(entry, out);
        }
        for section in &tree.sections {
            match section.label {
                Some(label) => writeln!(out, "{indent}-- {label}"),
                None => writeln!(out, "{indent}--"),
            }
            .unwrap();
            for entry in &section.items {
                item(entry, out);
            }
        }
    }

    fn text(menus: &Menus) -> String {
        let mut out = String::new();
        for (name, tree) in [
            ("main", &menus.main),
            ("tab", &menus.tab),
            ("editor", &menus.editor),
            ("line endings", &menus.line_endings),
        ] {
            let _ = writeln!(out, "# {name}");
            dump(tree, 0, &mut out);
        }
        out
    }

    #[test]
    fn the_menus_keep_their_structure() {
        insta::assert_snapshot!(text(&Menus::new(|_| true)));
    }

    #[test]
    fn unavailable_actions_leave_every_menu() {
        let hidden = [ActionId::OpenTerminalHere, ActionId::CheckForUpdates];
        let all = text(&Menus::new(|_| true));
        let some = text(&Menus::new(|id| !hidden.contains(&id)));
        let mut expected: Vec<&str> = all
            .lines()
            .filter(|line| {
                !hidden
                    .iter()
                    .any(|id| line.contains(&format!("[{}]", id.name())))
            })
            .collect();
        // Check for Updates is the only item of the first page's own section.
        let tab = expected.iter().position(|line| *line == "# tab").unwrap();
        assert_eq!(expected.remove(tab - 1), "--");
        assert_eq!(some.lines().collect::<Vec<_>>(), expected);
        assert_ne!(all, some);
        let none = Menus::new(|_| false);
        assert!(none.tab.sections.is_empty());
        assert!(none.main.sections.is_empty());
        assert_eq!(none.main.items.len(), Menu::ALL.len());
    }

    #[test]
    fn nested_submenus_follow_their_order() {
        let mut ranks: Vec<usize> = SUBMENU_ORDER
            .iter()
            .map(|name| submenu_rank(name))
            .collect();
        ranks.dedup();
        assert_eq!(ranks, (0..SUBMENU_ORDER.len()).collect::<Vec<_>>());
        let mut places: Vec<(&str, Menu, u8)> = ActionId::ALL
            .into_iter()
            .filter_map(|id| id.spec().menu)
            .filter_map(|place| Some((place.submenu?, place.menu, place.parent_section)))
            .collect();
        places.dedup();
        places.sort_unstable_by_key(|&(name, ..)| name);
        for pair in places.windows(2) {
            assert!(pair[0].0 != pair[1].0 || pair[0] == pair[1], "{pair:?}");
        }
        for (name, ..) in places {
            assert!(SUBMENU_ORDER.contains(&name), "{name} has no rank");
        }
    }

    #[test]
    fn parameterized_actions_in_menus_have_a_list() {
        for id in ActionId::ALL {
            if id.kind().takes_parameter() && id.spec().menu.is_some() {
                assert!(
                    matches!(entry(id), Some(MenuItem::List(..))),
                    "{id:?} has a menu place but no list"
                );
            }
        }
        let menus = Menus::new(|_| true);
        let file = |name: &str| {
            menus
                .main
                .items
                .iter()
                .find_map(|item| match item {
                    MenuItem::Submenu { label, menu } if *label == name => Some(menu.clone()),
                    _ => None,
                })
                .unwrap()
        };
        let recent = file("File")
            .sections
            .iter()
            .flat_map(|section| &section.items)
            .find_map(|item| match item {
                MenuItem::Submenu { label, menu } if *label == RECENT_SUBMENU => Some(menu.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(recent.lists().collect::<Vec<_>>(), [MenuList::RecentFiles]);
        assert_eq!(file("File").lists().count(), 0);
    }

    #[test]
    fn context_menus_use_registry_actions() {
        for section in TAB_MENU {
            for entry in *section {
                let (Fixed::Action(id) | Fixed::WhenEnabled(id)) = *entry else {
                    panic!("{entry:?}");
                };
                assert_eq!(id.kind(), ActionKind::Command, "{id:?}");
                assert_eq!(id.scope(), ActionScope::Window, "{id:?}");
                assert!(id.spec().palette, "{id:?}");
            }
        }
        for section in EDITOR_MENU {
            for entry in *section {
                match *entry {
                    Fixed::Submenu(name) => assert!(
                        ActionId::ALL.into_iter().any(|id| id
                            .spec()
                            .menu
                            .and_then(|place| place.submenu)
                            == Some(name)),
                        "{name}"
                    ),
                    Fixed::Action(id) | Fixed::WhenEnabled(id) => {
                        assert_eq!(id.key_scope(), KeyScope::Editor);
                    }
                }
            }
        }
    }
}
