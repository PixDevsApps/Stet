//! The menus built from the registry: the hamburger menu's places and nested submenus, and the
//! tab strip's and the editor's context menus.

use super::ActionId;

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
pub const RECENT_SUBMENU: &str = "Open Recent";

/// Edit › EOL Conversion.
pub const EOL_SUBMENU: &str = "EOL Conversion";

/// Encoding › Character Sets: every encoding, grouped, to reinterpret the file as.
pub const CHARACTER_SETS_SUBMENU: &str = "Character Sets";

/// Edit › Copy to Clipboard: the document's path and name.
pub const COPY_SUBMENU: &str = "Copy to Clipboard";

/// Edit › Convert Case to.
pub const CASE_SUBMENU: &str = "Convert Case to";

/// Edit › Line Operations.
pub const LINES_SUBMENU: &str = "Line Operations";

/// Edit › Comment/Uncomment.
pub const COMMENT_SUBMENU: &str = "Comment/Uncomment";

/// Edit › Blank Operations.
pub const BLANK_SUBMENU: &str = "Blank Operations";

/// Tools › JSON and Tools › XML.
pub const JSON_SUBMENU: &str = "JSON";
pub const XML_SUBMENU: &str = "XML";

/// Tools › Compare: the comparison commands.
pub const COMPARE_SUBMENU: &str = "Compare";

/// Search › Style All Occurrences of Token, Clear Style, Jump Up, Jump Down and Bookmark.
pub const STYLE_TOKEN_SUBMENU: &str = "Style All Occurrences of Token";
pub const CLEAR_STYLE_SUBMENU: &str = "Clear Style";
pub const JUMP_UP_SUBMENU: &str = "Jump Up";
pub const JUMP_DOWN_SUBMENU: &str = "Jump Down";
pub const BOOKMARK_SUBMENU: &str = "Bookmark";

/// View › Move/Clone Current Document.
pub const MOVE_CLONE_SUBMENU: &str = "Move/Clone Current Document";

/// Nested submenus in the order a section lists them.
pub const SUBMENU_ORDER: &[&str] = &[
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
pub fn submenu_rank(submenu: &str) -> usize {
    SUBMENU_ORDER
        .iter()
        .position(|name| *name == submenu)
        .unwrap_or(SUBMENU_ORDER.len())
}

/// The tab strip's context menu, by section; each action acts on the tab that was clicked.
pub const TAB_MENU: &[&[ActionId]] = &[
    // Only the one that applies shows.
    &[ActionId::PinTab, ActionId::UnpinTab],
    &[
        ActionId::CloseTab,
        ActionId::CloseOthers,
        ActionId::CloseToTheRight,
    ],
    &[ActionId::Save, ActionId::SaveAs],
    &[
        ActionId::CopyFullPath,
        ActionId::CopyFileName,
        ActionId::CopyDirectoryPath,
    ],
    &[ActionId::OpenContainingFolder, ActionId::OpenTerminalHere],
    &[ActionId::RenameFile, ActionId::MoveToTrash],
];

/// An entry of the editor's context menu, after GtkTextView's own clipboard items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorMenuItem {
    /// Every action of an Edit submenu, as the hamburger menu has it.
    Submenu(&'static str),
    Action(ActionId),
}

/// The editor's context menu, by section.
pub const EDITOR_MENU: &[&[EditorMenuItem]] = &[
    &[
        EditorMenuItem::Submenu(CASE_SUBMENU),
        EditorMenuItem::Submenu(COMMENT_SUBMENU),
        EditorMenuItem::Submenu(LINES_SUBMENU),
    ],
    &[
        EditorMenuItem::Action(ActionId::SelectAndFindNext),
        EditorMenuItem::Action(ActionId::GoToMatchingBrace),
    ],
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{ActionKind, KeyScope};

    #[test]
    fn the_edit_menu_keeps_its_order() {
        let submenus: Vec<&str> = ActionId::ALL
            .into_iter()
            .filter_map(|id| id.spec().menu)
            .filter(|place| place.menu == Menu::Edit && place.parent_section == 3)
            .filter_map(|place| place.submenu)
            .collect();
        let mut ordered = submenus.clone();
        ordered.sort_by_key(|name| submenu_rank(name));
        ordered.dedup();
        assert_eq!(
            ordered,
            [
                COPY_SUBMENU,
                CASE_SUBMENU,
                LINES_SUBMENU,
                COMMENT_SUBMENU,
                EOL_SUBMENU,
                BLANK_SUBMENU
            ]
        );
        assert_eq!(
            ActionId::DuplicateLine.menu_path().as_deref(),
            Some("Edit › Line Operations")
        );
        assert_eq!(
            ActionId::FormatJson.menu_path().as_deref(),
            Some("Tools › JSON")
        );
        assert!(SUBMENU_ORDER.iter().all(|name| submenu_rank(name) < 99));
    }

    #[test]
    fn context_menus_use_registry_actions() {
        for section in TAB_MENU {
            for id in *section {
                assert_eq!(id.kind(), ActionKind::Command, "{id:?}");
                assert!(id.spec().palette, "{id:?}");
            }
        }
        for section in EDITOR_MENU {
            for item in *section {
                match item {
                    EditorMenuItem::Submenu(name) => assert!(
                        ActionId::ALL.into_iter().any(|id| id
                            .spec()
                            .menu
                            .and_then(|place| place.submenu)
                            == Some(*name)),
                        "{name}"
                    ),
                    EditorMenuItem::Action(id) => assert_eq!(id.key_scope(), KeyScope::Editor),
                }
            }
        }
    }
}
