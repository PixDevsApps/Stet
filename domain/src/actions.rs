//! The action registry: every user-visible command with its GAction name, scope, label, menu
//! place, default keys (ADR-009) and palette visibility. The hamburger menu, the command palette
//! and the accelerators are all generated from it, so a menu entry cannot exist without a
//! working action.

/// Where an action lives: on the application, or on the window that has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionScope {
    App,
    Window,
}

impl ActionScope {
    /// The GTK action-group prefix: `app` or `win`.
    pub const fn prefix(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Window => "win",
        }
    }
}

/// Where an action's keys work (ADR-009 amendment of M5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyScope {
    /// Application accelerators: anywhere in the window, before the focused widget.
    Window,
    /// Only while an editor has the keyboard focus, before GtkSourceView's own bindings. The
    /// text tools use it, so their keys never change the text from the find bar or a list.
    Editor,
}

/// The hamburger menu's submenus, in the order the menu lists them.
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
}

impl Menu {
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
    const fn at(menu: Menu, section: u8) -> Option<Self> {
        Some(Self {
            menu,
            submenu: None,
            section,
            parent_section: 0,
        })
    }

    const fn nested(menu: Menu, submenu: &'static str, section: u8) -> Option<Self> {
        Self::nested_in(menu, 0, submenu, section)
    }

    const fn nested_in(
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

/// The shape of the GAction behind an [`ActionId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    /// A plain command.
    Command,
    /// A command with a string parameter (GVariant type `s`). Its menu entries, if any, are
    /// generated at run time, one per value (for example one per recent file).
    WithString,
    /// An on/off setting with a boolean state, shown with a check mark.
    Toggle,
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

    pub fn is_empty(self) -> bool {
        self.app.is_empty() && self.widget.is_empty()
    }
}

/// Everything the registry knows about one action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionSpec {
    /// The GAction name, stable across releases (keymaps and scripts refer to it).
    pub name: &'static str,
    pub scope: ActionScope,
    pub kind: ActionKind,
    pub label: &'static str,
    pub menu: Option<MenuPlace>,
    pub keys: Keys,
    pub key_scope: KeyScope,
    /// Listed in the command palette. Parameterized actions are reached through their own
    /// entries instead (recent files, the language picker).
    pub palette: bool,
}

/// Every user-visible command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionId {
    NewTab,
    Open,
    // Navigation (M8).
    QuickOpen,
    OpenRecent,
    ClearRecent,
    Save,
    SaveAs,
    SaveAll,
    CloseTab,
    CloseAll,
    RestoreClosedTab,
    // Sessions (M2).
    ForgetDrafts,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    Delete,
    SelectAll,
    Find,
    FindInFiles,
    FindNext,
    FindPrevious,
    Replace,
    ReplaceNext,
    ReplaceAll,
    ReplaceAllInOpenDocuments,
    Count,
    FindAllInDocument,
    FindAllInOpenDocuments,
    StopFindInFiles,
    // Replace in Files (M8).
    ReplaceInFiles,
    SearchResults,
    NextSearchResult,
    PreviousSearchResult,
    CopySearchResults,
    ClearSearchResults,
    CloseSearchResults,
    GoToLine,
    // Back and forward (M8).
    GoBack,
    GoForward,
    WordWrap,
    ShowWhitespace,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    FullScreen,
    NextTab,
    PreviousTab,
    // The MRU switcher (M8).
    NextRecentTab,
    PreviousRecentTab,
    MoveTabForward,
    MoveTabBackward,
    // Pinned tabs (M8).
    PinTab,
    UnpinTab,
    CommandPalette,
    ChooseLanguage,
    SetLanguage,
    // Files, encodings and line endings (M3).
    ReloadFromDisk,
    EolCrLf,
    EolLf,
    EolCr,
    EncodeUtf8,
    EncodeUtf8Bom,
    EncodeUtf16BeBom,
    EncodeUtf16LeBom,
    Reinterpret,
    ConvertToAnsi,
    ConvertToUtf8,
    ConvertToUtf8Bom,
    ConvertToUtf16BeBom,
    ConvertToUtf16LeBom,
    ConvertEncoding,
    ChooseEncoding,
    // Text toolbox and chrome (M5).
    RenameFile,
    MoveToTrash,
    OpenContainingFolder,
    OpenTerminalHere,
    CloseOthers,
    CloseToTheRight,
    CopyFullPath,
    CopyFileName,
    CopyDirectoryPath,
    Uppercase,
    Lowercase,
    ProperCase,
    ProperCaseBlend,
    SentenceCase,
    SentenceCaseBlend,
    InvertCase,
    DuplicateLine,
    CutLine,
    CopyLine,
    DeleteLine,
    TransposeLine,
    MoveLineUp,
    MoveLineDown,
    JoinLines,
    BlankLineAbove,
    BlankLineBelow,
    RemoveDuplicateLines,
    RemoveConsecutiveDuplicateLines,
    RemoveEmptyLines,
    RemoveBlankLines,
    ReverseLines,
    SortLexicalAscending,
    SortLexicalDescending,
    SortIgnoreCaseAscending,
    SortIgnoreCaseDescending,
    SortIntegerAscending,
    SortIntegerDescending,
    SortDecimalCommaAscending,
    SortDecimalCommaDescending,
    SortDecimalDotAscending,
    SortDecimalDotDescending,
    SortLengthAscending,
    SortLengthDescending,
    ToggleComment,
    CommentLines,
    UncommentLines,
    ToggleBlockComment,
    TrimTrailing,
    TrimLeading,
    TrimBoth,
    TabsToSpaces,
    SpacesToTabs,
    SpacesToTabsLeading,
    AddPrefixSuffix,
    InsertNumbers,
    GoToMatchingBrace,
    SelectToMatchingBrace,
    SelectAndFindNext,
    SelectAndFindPrevious,
    DocumentMap,
    ChooseIndentation,
    DetectIndentation,
    OpenSettings,
    OpenKeyboardShortcuts,
    SetAsDefaultEditor,
    FormatJson,
    MinifyJson,
    ValidateJson,
    FormatXml,
    ValidateXml,
    KeyboardShortcuts,
    About,
    // Column mode (M6).
    ColumnSelectLeft,
    ColumnSelectRight,
    ColumnSelectUp,
    ColumnSelectDown,
    ColumnSelectLineStart,
    ColumnSelectLineEnd,
    ColumnSelectPageUp,
    ColumnSelectPageDown,
    ColumnBeginEndSelect,
    ColumnEditor,
    // Bookmarks (M7): Search › Bookmark.
    ToggleBookmark,
    NextBookmark,
    PreviousBookmark,
    ClearBookmarks,
    CutBookmarkedLines,
    CopyBookmarkedLines,
    PasteToBookmarkedLines,
    RemoveBookmarkedLines,
    RemoveUnbookmarkedLines,
    InverseBookmarks,
    // Mark and style tokens (M7).
    Mark,
    MarkAll,
    ClearMarks,
    CopyMarkedText,
    StyleToken1,
    StyleToken2,
    StyleToken3,
    StyleToken4,
    StyleToken5,
    ClearStyle1,
    ClearStyle2,
    ClearStyle3,
    ClearStyle4,
    ClearStyle5,
    ClearAllStyles,
    JumpUp1,
    JumpUp2,
    JumpUp3,
    JumpUp4,
    JumpUp5,
    JumpUpMark,
    JumpDown1,
    JumpDown2,
    JumpDown3,
    JumpDown4,
    JumpDown5,
    JumpDownMark,
    // Split view (M7).
    MoveToOtherView,
    CloneToOtherView,
    SwitchView,
    SyncVerticalScrolling,
    SyncHorizontalScrolling,
    // Compare (M7): Tools › Compare.
    Compare,
    CompareWithFile,
    CompareWithClipboard,
    CompareWithSaved,
    PreviousDifference,
    NextDifference,
    CompareIgnoreWhitespace,
    CompareIgnoreCase,
    ClearCompare,
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

/// Tools › Compare: the comparison commands (M7).
pub const COMPARE_SUBMENU: &str = "Compare";

/// Search › Style All Occurrences of Token, Clear Style, Jump Up, Jump Down and Bookmark (M7).
pub const STYLE_TOKEN_SUBMENU: &str = "Style All Occurrences of Token";
pub const CLEAR_STYLE_SUBMENU: &str = "Clear Style";
pub const JUMP_UP_SUBMENU: &str = "Jump Up";
pub const JUMP_DOWN_SUBMENU: &str = "Jump Down";
pub const BOOKMARK_SUBMENU: &str = "Bookmark";

/// View › Move/Clone Current Document (M7).
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
    // Only the one that applies shows (M8).
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

/// GtkSourceView and GtkTextView key bindings that clash with Stet's keymap or with keys
/// later milestones need (ADR-009): `change-number` (and GtkTextView's unselect-all
/// on Ctrl+Shift+A), `move-lines`, `move-words` (M8's back and forward), `move-viewport` (M6's
/// column selection), and GtkTextView's select-all and unselect-all on Ctrl+/ and Ctrl+\. An
/// editor-scoped controller in the capture phase takes these keys before the view, so they do
/// nothing unless the keymap gives them to an action. The keypad variants are GtkSourceView's.
pub const OVERRIDDEN_BUILTINS: &[&str] = &[
    "<Control><Shift>x",
    "<Control><Shift>a",
    "<Alt>Up",
    "<Alt>KP_Up",
    "<Alt>Down",
    "<Alt>KP_Down",
    "<Alt>Left",
    "<Alt>KP_Left",
    "<Alt>Right",
    "<Alt>KP_Right",
    "<Alt><Shift>Up",
    "<Alt><Shift>KP_Up",
    "<Alt><Shift>Down",
    "<Alt><Shift>KP_Down",
    "<Alt><Shift>Page_Up",
    "<Alt><Shift>KP_Page_Up",
    "<Alt><Shift>Page_Down",
    "<Alt><Shift>KP_Next",
    "<Alt><Shift>Home",
    "<Alt><Shift>KP_Home",
    "<Alt><Shift>End",
    "<Alt><Shift>KP_End",
    "<Control>slash",
    "<Control>backslash",
];

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

/// A command in one of the Search menu's marking submenus (M7).
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

/// A Jump Up or Jump Down command (M7). It moves the editor's selection, so its keys work in
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

/// A command in Tools › Compare (M7).
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

impl ActionId {
    pub const ALL: [Self; 208] = [
        Self::NewTab,
        Self::Open,
        Self::QuickOpen,
        Self::OpenRecent,
        Self::ClearRecent,
        Self::Save,
        Self::SaveAs,
        Self::SaveAll,
        Self::CloseTab,
        Self::CloseAll,
        Self::RestoreClosedTab,
        Self::ForgetDrafts,
        Self::Quit,
        Self::Undo,
        Self::Redo,
        Self::Cut,
        Self::Copy,
        Self::Paste,
        Self::Delete,
        Self::SelectAll,
        Self::Find,
        Self::FindInFiles,
        Self::FindNext,
        Self::FindPrevious,
        Self::Replace,
        Self::ReplaceNext,
        Self::ReplaceAll,
        Self::ReplaceAllInOpenDocuments,
        Self::Count,
        Self::FindAllInDocument,
        Self::FindAllInOpenDocuments,
        Self::StopFindInFiles,
        Self::ReplaceInFiles,
        Self::SearchResults,
        Self::NextSearchResult,
        Self::PreviousSearchResult,
        Self::CopySearchResults,
        Self::ClearSearchResults,
        Self::CloseSearchResults,
        Self::GoToLine,
        Self::GoBack,
        Self::GoForward,
        Self::WordWrap,
        Self::ShowWhitespace,
        Self::ZoomIn,
        Self::ZoomOut,
        Self::ZoomReset,
        Self::FullScreen,
        Self::NextTab,
        Self::PreviousTab,
        Self::NextRecentTab,
        Self::PreviousRecentTab,
        Self::MoveTabForward,
        Self::MoveTabBackward,
        Self::PinTab,
        Self::UnpinTab,
        Self::CommandPalette,
        Self::ChooseLanguage,
        Self::SetLanguage,
        Self::ReloadFromDisk,
        Self::EolCrLf,
        Self::EolLf,
        Self::EolCr,
        Self::EncodeUtf8,
        Self::EncodeUtf8Bom,
        Self::EncodeUtf16BeBom,
        Self::EncodeUtf16LeBom,
        Self::Reinterpret,
        Self::ConvertToAnsi,
        Self::ConvertToUtf8,
        Self::ConvertToUtf8Bom,
        Self::ConvertToUtf16BeBom,
        Self::ConvertToUtf16LeBom,
        Self::ConvertEncoding,
        Self::ChooseEncoding,
        Self::RenameFile,
        Self::MoveToTrash,
        Self::OpenContainingFolder,
        Self::OpenTerminalHere,
        Self::CloseOthers,
        Self::CloseToTheRight,
        Self::CopyFullPath,
        Self::CopyFileName,
        Self::CopyDirectoryPath,
        Self::Uppercase,
        Self::Lowercase,
        Self::ProperCase,
        Self::ProperCaseBlend,
        Self::SentenceCase,
        Self::SentenceCaseBlend,
        Self::InvertCase,
        Self::DuplicateLine,
        Self::CutLine,
        Self::CopyLine,
        Self::DeleteLine,
        Self::TransposeLine,
        Self::MoveLineUp,
        Self::MoveLineDown,
        Self::JoinLines,
        Self::BlankLineAbove,
        Self::BlankLineBelow,
        Self::RemoveDuplicateLines,
        Self::RemoveConsecutiveDuplicateLines,
        Self::RemoveEmptyLines,
        Self::RemoveBlankLines,
        Self::ReverseLines,
        Self::SortLexicalAscending,
        Self::SortLexicalDescending,
        Self::SortIgnoreCaseAscending,
        Self::SortIgnoreCaseDescending,
        Self::SortIntegerAscending,
        Self::SortIntegerDescending,
        Self::SortDecimalCommaAscending,
        Self::SortDecimalCommaDescending,
        Self::SortDecimalDotAscending,
        Self::SortDecimalDotDescending,
        Self::SortLengthAscending,
        Self::SortLengthDescending,
        Self::ToggleComment,
        Self::CommentLines,
        Self::UncommentLines,
        Self::ToggleBlockComment,
        Self::TrimTrailing,
        Self::TrimLeading,
        Self::TrimBoth,
        Self::TabsToSpaces,
        Self::SpacesToTabs,
        Self::SpacesToTabsLeading,
        Self::AddPrefixSuffix,
        Self::InsertNumbers,
        Self::GoToMatchingBrace,
        Self::SelectToMatchingBrace,
        Self::SelectAndFindNext,
        Self::SelectAndFindPrevious,
        Self::DocumentMap,
        Self::ChooseIndentation,
        Self::DetectIndentation,
        Self::OpenSettings,
        Self::OpenKeyboardShortcuts,
        Self::SetAsDefaultEditor,
        Self::FormatJson,
        Self::MinifyJson,
        Self::ValidateJson,
        Self::FormatXml,
        Self::ValidateXml,
        Self::KeyboardShortcuts,
        Self::About,
        Self::ColumnSelectLeft,
        Self::ColumnSelectRight,
        Self::ColumnSelectUp,
        Self::ColumnSelectDown,
        Self::ColumnSelectLineStart,
        Self::ColumnSelectLineEnd,
        Self::ColumnSelectPageUp,
        Self::ColumnSelectPageDown,
        Self::ColumnBeginEndSelect,
        Self::ColumnEditor,
        Self::ToggleBookmark,
        Self::NextBookmark,
        Self::PreviousBookmark,
        Self::ClearBookmarks,
        Self::CutBookmarkedLines,
        Self::CopyBookmarkedLines,
        Self::PasteToBookmarkedLines,
        Self::RemoveBookmarkedLines,
        Self::RemoveUnbookmarkedLines,
        Self::InverseBookmarks,
        Self::Mark,
        Self::MarkAll,
        Self::ClearMarks,
        Self::CopyMarkedText,
        Self::StyleToken1,
        Self::StyleToken2,
        Self::StyleToken3,
        Self::StyleToken4,
        Self::StyleToken5,
        Self::ClearStyle1,
        Self::ClearStyle2,
        Self::ClearStyle3,
        Self::ClearStyle4,
        Self::ClearStyle5,
        Self::ClearAllStyles,
        Self::JumpUp1,
        Self::JumpUp2,
        Self::JumpUp3,
        Self::JumpUp4,
        Self::JumpUp5,
        Self::JumpUpMark,
        Self::JumpDown1,
        Self::JumpDown2,
        Self::JumpDown3,
        Self::JumpDown4,
        Self::JumpDown5,
        Self::JumpDownMark,
        Self::MoveToOtherView,
        Self::CloneToOtherView,
        Self::SwitchView,
        Self::SyncVerticalScrolling,
        Self::SyncHorizontalScrolling,
        Self::Compare,
        Self::CompareWithFile,
        Self::CompareWithClipboard,
        Self::CompareWithSaved,
        Self::PreviousDifference,
        Self::NextDifference,
        Self::CompareIgnoreWhitespace,
        Self::CompareIgnoreCase,
        Self::ClearCompare,
    ];

    pub const fn spec(self) -> ActionSpec {
        use Menu::*;
        match self {
            Self::NewTab => command(
                "new-tab",
                "New",
                MenuPlace::at(File, 0),
                Keys::app(&["<Control>n"]),
            ),
            Self::Open => command(
                "open",
                "Open…",
                MenuPlace::at(File, 0),
                Keys::app(&["<Control>o"]),
            ),
            // Ctrl+P is quick open, as in VS Code and Sublime Text, not Print, which is 1.x
            // (ADR-009 amendment, M8).
            Self::QuickOpen => command(
                "quick-open",
                "Quick Open…",
                MenuPlace::at(File, 0),
                Keys::app(&["<Control>p"]),
            ),
            Self::OpenRecent => ActionSpec {
                kind: ActionKind::WithString,
                palette: false,
                ..command(
                    "open-recent",
                    "Open Recent",
                    MenuPlace::nested(File, RECENT_SUBMENU, 0),
                    Keys::NONE,
                )
            },
            Self::ClearRecent => command(
                "clear-recent",
                "Clear Recent Files",
                MenuPlace::nested(File, RECENT_SUBMENU, 1),
                Keys::NONE,
            ),
            Self::Save => command(
                "save",
                "Save",
                MenuPlace::at(File, 1),
                Keys::app(&["<Control>s"]),
            ),
            Self::SaveAs => command(
                "save-as",
                "Save As…",
                MenuPlace::at(File, 1),
                Keys::app(&["<Control><Alt>s"]),
            ),
            Self::SaveAll => command(
                "save-all",
                "Save All",
                MenuPlace::at(File, 1),
                Keys::app(&["<Control><Shift>s"]),
            ),
            Self::CloseTab => command(
                "close-tab",
                "Close",
                MenuPlace::at(File, 3),
                Keys::app(&["<Control>w"]),
            ),
            Self::CloseAll => command(
                "close-all",
                "Close All",
                MenuPlace::at(File, 3),
                Keys::app(&["<Control><Shift>w"]),
            ),
            Self::RestoreClosedTab => command(
                "restore-closed-tab",
                "Restore Closed Tab",
                MenuPlace::at(File, 3),
                Keys::app(&["<Control><Shift>t"]),
            ),
            // Discards every unsaved draft and its backup, after a confirmation (ADR-006).
            Self::ForgetDrafts => command(
                "forget-drafts",
                "Forget Unsaved Drafts…",
                MenuPlace::at(File, 3),
                Keys::NONE,
            ),
            // Ctrl+Q stays free for Toggle Single Line Comment (M5).
            Self::Quit => ActionSpec {
                scope: ActionScope::App,
                ..command(
                    "quit",
                    "Quit",
                    MenuPlace::at(File, 4),
                    Keys::app(&["<Control><Alt>q", "<Alt>F4"]),
                )
            },
            Self::Undo => command(
                "undo",
                "Undo",
                MenuPlace::at(Edit, 0),
                Keys::widget(&["<Control>z"]),
            ),
            Self::Redo => command(
                "redo",
                "Redo",
                MenuPlace::at(Edit, 0),
                Keys::widget(&["<Control>y", "<Control><Shift>z"]),
            ),
            Self::Cut => command(
                "cut",
                "Cut",
                MenuPlace::at(Edit, 1),
                Keys::widget(&["<Control>x", "<Shift>Delete"]),
            ),
            Self::Copy => command(
                "copy",
                "Copy",
                MenuPlace::at(Edit, 1),
                Keys::widget(&["<Control>c", "<Control>Insert"]),
            ),
            Self::Paste => command(
                "paste",
                "Paste",
                MenuPlace::at(Edit, 1),
                Keys::widget(&["<Control>v", "<Shift>Insert"]),
            ),
            Self::Delete => command(
                "delete",
                "Delete",
                MenuPlace::at(Edit, 1),
                Keys::widget(&["Delete"]),
            ),
            Self::SelectAll => command(
                "select-all",
                "Select All",
                MenuPlace::at(Edit, 2),
                Keys::widget(&["<Control>a"]),
            ),
            Self::Find => command(
                "find",
                "Find…",
                MenuPlace::at(Search, 0),
                Keys::app(&["<Control>f"]),
            ),
            // The F-key-free key comes first so that menus show it (ADR-009 amendment).
            Self::FindNext => command(
                "find-next",
                "Find Next",
                MenuPlace::at(Search, 0),
                Keys::app(&["<Alt>Down", "F3"]),
            ),
            Self::FindPrevious => command(
                "find-previous",
                "Find Previous",
                MenuPlace::at(Search, 0),
                Keys::app(&["<Alt>Up", "<Shift>F3"]),
            ),
            Self::FindInFiles => command(
                "find-in-files",
                "Find in Files…",
                MenuPlace::at(Search, 0),
                Keys::app(&["<Control><Shift>f"]),
            ),
            Self::Replace => command(
                "replace",
                "Replace…",
                MenuPlace::at(Search, 0),
                Keys::app(&["<Control>h"]),
            ),
            // The find bar's buttons: no keys or menu entries, but the palette reaches them.
            Self::ReplaceNext => command("replace-next", "Replace Next", None, Keys::NONE),
            Self::ReplaceAll => command("replace-all", "Replace All", None, Keys::NONE),
            Self::ReplaceAllInOpenDocuments => command(
                "replace-all-in-open-documents",
                "Replace All in Open Documents",
                None,
                Keys::NONE,
            ),
            Self::Count => command("count", "Count", None, Keys::NONE),
            Self::FindAllInDocument => command(
                "find-all-in-document",
                "Find All in Current Document",
                None,
                Keys::NONE,
            ),
            Self::FindAllInOpenDocuments => command(
                "find-all-in-open-documents",
                "Find All in Open Documents",
                None,
                Keys::NONE,
            ),
            Self::StopFindInFiles => {
                command("stop-find-in-files", "Stop Find in Files", None, Keys::NONE)
            }
            // The Find in Files row's other button.
            Self::ReplaceInFiles => {
                command("replace-in-files", "Replace in Files", None, Keys::NONE)
            }
            // F7, F4 and Shift+F4 after an F-key-free key (ADR-009 amendment of 2026-10-01, M4).
            Self::SearchResults => command(
                "search-results",
                "Search Results Window",
                MenuPlace::at(Search, 1),
                Keys::app(&["<Control><Alt>r", "F7"]),
            ),
            Self::NextSearchResult => command(
                "next-search-result",
                "Next Search Result",
                MenuPlace::at(Search, 1),
                Keys::app(&["<Control><Alt>Down", "F4"]),
            ),
            Self::PreviousSearchResult => command(
                "previous-search-result",
                "Previous Search Result",
                MenuPlace::at(Search, 1),
                Keys::app(&["<Control><Alt>Up", "<Shift>F4"]),
            ),
            Self::CopySearchResults => command(
                "copy-search-results",
                "Copy Search Results",
                None,
                Keys::NONE,
            ),
            Self::ClearSearchResults => command(
                "clear-search-results",
                "Clear Search Results",
                None,
                Keys::NONE,
            ),
            Self::CloseSearchResults => command(
                "close-search-results",
                "Close Search Results",
                None,
                Keys::NONE,
            ),
            Self::GoToLine => command(
                "go-to-line",
                "Go to Line…",
                MenuPlace::at(Search, 2),
                Keys::app(&["<Control>g"]),
            ),
            // Back and forward through the places jumps left (M8): the browsers' Alt+Left and
            // Alt+Right, which GtkSourceView would use to move words (ADR-009 amendment).
            Self::GoBack => command(
                "go-back",
                "Go Back",
                MenuPlace::at(Search, 2),
                Keys::app(&["<Alt>Left"]),
            ),
            Self::GoForward => command(
                "go-forward",
                "Go Forward",
                MenuPlace::at(Search, 2),
                Keys::app(&["<Alt>Right"]),
            ),
            Self::WordWrap => toggle("word-wrap", "Word Wrap", MenuPlace::at(View, 0)),
            Self::ShowWhitespace => {
                toggle("show-whitespace", "Show Whitespace", MenuPlace::at(View, 0))
            }
            // `+` is unshifted on the Norwegian layout and `=` on the US one.
            Self::ZoomIn => command(
                "zoom-in",
                "Zoom In",
                MenuPlace::at(View, 1),
                Keys::app(&["<Control>plus", "<Control>equal", "<Control>KP_Add"]),
            ),
            Self::ZoomOut => command(
                "zoom-out",
                "Zoom Out",
                MenuPlace::at(View, 1),
                Keys::app(&["<Control>minus", "<Control>KP_Subtract"]),
            ),
            Self::ZoomReset => command(
                "zoom-reset",
                "Reset Zoom",
                MenuPlace::at(View, 1),
                Keys::app(&["<Control>0", "<Control>KP_Divide"]),
            ),
            Self::FullScreen => ActionSpec {
                keys: Keys::app(&["<Alt>Return", "F11"]),
                ..toggle("full-screen", "Full Screen", MenuPlace::at(View, 2))
            },
            // Tab order on Ctrl+PgDn and Ctrl+PgUp; Ctrl+Tab is the MRU switcher.
            Self::NextTab => command(
                "next-tab",
                "Next Tab",
                MenuPlace::at(View, 3),
                Keys::app(&["<Control>Page_Down"]),
            ),
            Self::PreviousTab => command(
                "previous-tab",
                "Previous Tab",
                MenuPlace::at(View, 3),
                Keys::app(&["<Control>Page_Up"]),
            ),
            // Most recently used order while Ctrl is held; a tap goes to the previous tab
            // (M8, ADR-009 amendment).
            Self::NextRecentTab => command(
                "next-recent-tab",
                "Next Recent Tab",
                MenuPlace::at(View, 3),
                Keys::app(&["<Control>Tab"]),
            ),
            Self::PreviousRecentTab => command(
                "previous-recent-tab",
                "Previous Recent Tab",
                MenuPlace::at(View, 3),
                Keys::app(&["<Control><Shift>Tab"]),
            ),
            Self::MoveTabForward => command(
                "move-tab-forward",
                "Move Tab Forward",
                MenuPlace::at(View, 3),
                Keys::app(&["<Control><Shift>Page_Down"]),
            ),
            Self::MoveTabBackward => command(
                "move-tab-backward",
                "Move Tab Backward",
                MenuPlace::at(View, 3),
                Keys::app(&["<Control><Shift>Page_Up"]),
            ),
            // The tab menu's pin entries; only the one that applies is enabled (M8).
            Self::PinTab => command("pin-tab", "Pin Tab", MenuPlace::at(View, 3), Keys::NONE),
            Self::UnpinTab => command("unpin-tab", "Unpin Tab", MenuPlace::at(View, 3), Keys::NONE),
            Self::CommandPalette => command(
                "command-palette",
                "Command Palette…",
                MenuPlace::at(View, 5),
                Keys::app(&["<Control><Shift>p", "F1"]),
            ),
            Self::ChooseLanguage => command(
                "choose-language",
                "Set Language…",
                MenuPlace::at(Language, 0),
                Keys::NONE,
            ),
            Self::SetLanguage => ActionSpec {
                kind: ActionKind::WithString,
                palette: false,
                ..command("set-language", "Set Language", None, Keys::NONE)
            },
            Self::ReloadFromDisk => command(
                "reload-from-disk",
                "Reload from Disk",
                MenuPlace::at(File, 0),
                Keys::NONE,
            ),
            // Edit › EOL Conversion; the buffer keeps LF (ADR-008).
            Self::EolCrLf => command(
                "eol-crlf",
                "Windows (CR LF)",
                MenuPlace::nested_in(Edit, 3, EOL_SUBMENU, 0),
                Keys::NONE,
            ),
            Self::EolLf => command(
                "eol-lf",
                "Unix (LF)",
                MenuPlace::nested_in(Edit, 3, EOL_SUBMENU, 0),
                Keys::NONE,
            ),
            Self::EolCr => command(
                "eol-cr",
                "Macintosh (CR)",
                MenuPlace::nested_in(Edit, 3, EOL_SUBMENU, 0),
                Keys::NONE,
            ),
            // The Encoding menu: reinterpret the file's bytes ("Encode in"), any character
            // set, then convert what the next save writes (ADR-007).
            Self::EncodeUtf8 => command(
                "encode-utf8",
                "Encode in UTF-8",
                MenuPlace::at(Encoding, 0),
                Keys::NONE,
            ),
            Self::EncodeUtf8Bom => command(
                "encode-utf8-bom",
                "Encode in UTF-8-BOM",
                MenuPlace::at(Encoding, 0),
                Keys::NONE,
            ),
            Self::EncodeUtf16BeBom => command(
                "encode-utf16be-bom",
                "Encode in UTF-16 BE BOM",
                MenuPlace::at(Encoding, 0),
                Keys::NONE,
            ),
            Self::EncodeUtf16LeBom => command(
                "encode-utf16le-bom",
                "Encode in UTF-16 LE BOM",
                MenuPlace::at(Encoding, 0),
                Keys::NONE,
            ),
            Self::Reinterpret => with_encoding(
                "reinterpret",
                "Reinterpret As",
                MenuPlace::nested(Encoding, CHARACTER_SETS_SUBMENU, 0),
            ),
            Self::ConvertToAnsi => command(
                "convert-to-ansi",
                "Convert to ANSI (Windows-1252)",
                MenuPlace::at(Encoding, 1),
                Keys::NONE,
            ),
            Self::ConvertToUtf8 => command(
                "convert-to-utf8",
                "Convert to UTF-8",
                MenuPlace::at(Encoding, 1),
                Keys::NONE,
            ),
            Self::ConvertToUtf8Bom => command(
                "convert-to-utf8-bom",
                "Convert to UTF-8-BOM",
                MenuPlace::at(Encoding, 1),
                Keys::NONE,
            ),
            Self::ConvertToUtf16BeBom => command(
                "convert-to-utf16be-bom",
                "Convert to UTF-16 BE BOM",
                MenuPlace::at(Encoding, 1),
                Keys::NONE,
            ),
            Self::ConvertToUtf16LeBom => command(
                "convert-to-utf16le-bom",
                "Convert to UTF-16 LE BOM",
                MenuPlace::at(Encoding, 1),
                Keys::NONE,
            ),
            Self::ConvertEncoding => with_encoding("convert-encoding", "Convert To", None),
            Self::ChooseEncoding => command("choose-encoding", "Encoding…", None, Keys::NONE),
            // The tab strip's context menu and its File-menu places.
            Self::RenameFile => {
                command("rename-file", "Rename…", MenuPlace::at(File, 1), Keys::NONE)
            }
            Self::MoveToTrash => command(
                "move-to-trash",
                "Move to Trash…",
                MenuPlace::at(File, 3),
                Keys::NONE,
            ),
            Self::OpenContainingFolder => command(
                "open-containing-folder",
                "Open Containing Folder",
                MenuPlace::at(File, 2),
                Keys::NONE,
            ),
            Self::OpenTerminalHere => command(
                "open-terminal-here",
                "Open Terminal Here",
                MenuPlace::at(File, 2),
                Keys::NONE,
            ),
            Self::CloseOthers => command(
                "close-others",
                "Close Others",
                MenuPlace::at(File, 3),
                Keys::NONE,
            ),
            Self::CloseToTheRight => command(
                "close-to-the-right",
                "Close to the Right",
                MenuPlace::at(File, 3),
                Keys::NONE,
            ),
            Self::CopyFullPath => command(
                "copy-full-path",
                "Copy Full Path",
                MenuPlace::nested_in(Edit, 3, COPY_SUBMENU, 0),
                Keys::NONE,
            ),
            Self::CopyFileName => command(
                "copy-file-name",
                "Copy File Name",
                MenuPlace::nested_in(Edit, 3, COPY_SUBMENU, 0),
                Keys::NONE,
            ),
            Self::CopyDirectoryPath => command(
                "copy-directory-path",
                "Copy Directory Path",
                MenuPlace::nested_in(Edit, 3, COPY_SUBMENU, 0),
                Keys::NONE,
            ),
            // fcitx5 takes Ctrl+Shift+U and Ctrl+Alt+Shift+U, so UPPERCASE is Alt+Shift+U and
            // the blend variants have no key (ADR-009 amendment of M5).
            Self::Uppercase => editing(
                "uppercase",
                "UPPERCASE",
                MenuPlace::nested_in(Edit, 3, CASE_SUBMENU, 0),
                Keys::app(&["<Alt><Shift>u"]),
            ),
            Self::Lowercase => editing(
                "lowercase",
                "lowercase",
                MenuPlace::nested_in(Edit, 3, CASE_SUBMENU, 0),
                Keys::app(&["<Control>u"]),
            ),
            Self::ProperCase => editing(
                "proper-case",
                "Proper Case",
                MenuPlace::nested_in(Edit, 3, CASE_SUBMENU, 0),
                Keys::app(&["<Alt>u"]),
            ),
            Self::ProperCaseBlend => editing(
                "proper-case-blend",
                "Proper Case (blend)",
                MenuPlace::nested_in(Edit, 3, CASE_SUBMENU, 0),
                Keys::NONE,
            ),
            Self::SentenceCase => editing(
                "sentence-case",
                "Sentence case",
                MenuPlace::nested_in(Edit, 3, CASE_SUBMENU, 0),
                Keys::app(&["<Control><Alt>u"]),
            ),
            Self::SentenceCaseBlend => editing(
                "sentence-case-blend",
                "Sentence case (blend)",
                MenuPlace::nested_in(Edit, 3, CASE_SUBMENU, 0),
                Keys::NONE,
            ),
            Self::InvertCase => editing(
                "invert-case",
                "iNVERT cASE",
                MenuPlace::nested_in(Edit, 3, CASE_SUBMENU, 0),
                Keys::NONE,
            ),
            Self::DuplicateLine => editing(
                "duplicate-line",
                "Duplicate Current Line",
                MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 0),
                Keys::app(&["<Control>d"]),
            ),
            Self::CutLine => editing(
                "cut-line",
                "Cut Current Line",
                MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 0),
                Keys::app(&["<Control>l"]),
            ),
            Self::CopyLine => editing(
                "copy-line",
                "Copy Current Line",
                MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 0),
                Keys::app(&["<Control><Shift>x"]),
            ),
            Self::DeleteLine => editing(
                "delete-line",
                "Delete Current Line",
                MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 0),
                Keys::app(&["<Control><Shift>l"]),
            ),
            Self::TransposeLine => editing(
                "transpose-line",
                "Transpose Current Line",
                MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 0),
                Keys::app(&["<Control>t"]),
            ),
            Self::MoveLineUp => editing(
                "move-line-up",
                "Move Up Current Line",
                MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 1),
                Keys::app(&["<Control><Shift>Up"]),
            ),
            Self::MoveLineDown => editing(
                "move-line-down",
                "Move Down Current Line",
                MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 1),
                Keys::app(&["<Control><Shift>Down"]),
            ),
            Self::JoinLines => editing(
                "join-lines",
                "Join Lines",
                MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 1),
                Keys::app(&["<Control>j"]),
            ),
            Self::BlankLineAbove => editing(
                "blank-line-above",
                "Insert Blank Line Above Current",
                MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 1),
                Keys::app(&["<Control><Alt>Return"]),
            ),
            Self::BlankLineBelow => editing(
                "blank-line-below",
                "Insert Blank Line Below Current",
                MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 1),
                Keys::app(&["<Control><Alt><Shift>Return"]),
            ),
            Self::RemoveDuplicateLines => editing(
                "remove-duplicate-lines",
                "Remove Duplicate Lines",
                MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 2),
                Keys::NONE,
            ),
            Self::RemoveConsecutiveDuplicateLines => editing(
                "remove-consecutive-duplicate-lines",
                "Remove Consecutive Duplicate Lines",
                MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 2),
                Keys::NONE,
            ),
            Self::RemoveEmptyLines => editing(
                "remove-empty-lines",
                "Remove Empty Lines",
                MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 2),
                Keys::NONE,
            ),
            Self::RemoveBlankLines => editing(
                "remove-blank-lines",
                "Remove Empty Lines (Containing Blank Characters)",
                MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 2),
                Keys::NONE,
            ),
            Self::ReverseLines => editing(
                "reverse-lines",
                "Reverse Line Order",
                MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 2),
                Keys::NONE,
            ),
            Self::SortLexicalAscending => sort(
                "sort-lexical-ascending",
                "Sort Lines Lexicographically Ascending",
            ),
            Self::SortLexicalDescending => sort(
                "sort-lexical-descending",
                "Sort Lines Lexicographically Descending",
            ),
            Self::SortIgnoreCaseAscending => sort(
                "sort-ignore-case-ascending",
                "Sort Lines Lex. Ascending Ignoring Case",
            ),
            Self::SortIgnoreCaseDescending => sort(
                "sort-ignore-case-descending",
                "Sort Lines Lex. Descending Ignoring Case",
            ),
            Self::SortIntegerAscending => {
                sort("sort-integer-ascending", "Sort Lines As Integers Ascending")
            }
            Self::SortIntegerDescending => sort(
                "sort-integer-descending",
                "Sort Lines As Integers Descending",
            ),
            Self::SortDecimalCommaAscending => sort(
                "sort-decimal-comma-ascending",
                "Sort Lines As Decimals (Comma) Ascending",
            ),
            Self::SortDecimalCommaDescending => sort(
                "sort-decimal-comma-descending",
                "Sort Lines As Decimals (Comma) Descending",
            ),
            Self::SortDecimalDotAscending => sort(
                "sort-decimal-dot-ascending",
                "Sort Lines As Decimals (Dot) Ascending",
            ),
            Self::SortDecimalDotDescending => sort(
                "sort-decimal-dot-descending",
                "Sort Lines As Decimals (Dot) Descending",
            ),
            Self::SortLengthAscending => {
                sort("sort-length-ascending", "Sort Lines By Length Ascending")
            }
            Self::SortLengthDescending => {
                sort("sort-length-descending", "Sort Lines By Length Descending")
            }
            Self::ToggleComment => editing(
                "toggle-comment",
                "Toggle Single Line Comment",
                MenuPlace::nested_in(Edit, 3, COMMENT_SUBMENU, 0),
                Keys::app(&["<Control>q"]),
            ),
            Self::CommentLines => editing(
                "comment-lines",
                "Single Line Comment",
                MenuPlace::nested_in(Edit, 3, COMMENT_SUBMENU, 0),
                Keys::app(&["<Control>k"]),
            ),
            Self::UncommentLines => editing(
                "uncomment-lines",
                "Single Line Uncomment",
                MenuPlace::nested_in(Edit, 3, COMMENT_SUBMENU, 0),
                Keys::app(&["<Control><Shift>k"]),
            ),
            Self::ToggleBlockComment => editing(
                "toggle-block-comment",
                "Block Comment",
                MenuPlace::nested_in(Edit, 3, COMMENT_SUBMENU, 1),
                Keys::app(&["<Control><Shift>q"]),
            ),
            Self::TrimTrailing => editing(
                "trim-trailing",
                "Trim Trailing Space",
                MenuPlace::nested_in(Edit, 3, BLANK_SUBMENU, 0),
                Keys::NONE,
            ),
            Self::TrimLeading => editing(
                "trim-leading",
                "Trim Leading Space",
                MenuPlace::nested_in(Edit, 3, BLANK_SUBMENU, 0),
                Keys::NONE,
            ),
            Self::TrimBoth => editing(
                "trim-both",
                "Trim Leading and Trailing Space",
                MenuPlace::nested_in(Edit, 3, BLANK_SUBMENU, 0),
                Keys::NONE,
            ),
            Self::TabsToSpaces => editing(
                "tabs-to-spaces",
                "TAB to Space",
                MenuPlace::nested_in(Edit, 3, BLANK_SUBMENU, 1),
                Keys::NONE,
            ),
            Self::SpacesToTabs => editing(
                "spaces-to-tabs",
                "Space to TAB (All)",
                MenuPlace::nested_in(Edit, 3, BLANK_SUBMENU, 1),
                Keys::NONE,
            ),
            Self::SpacesToTabsLeading => editing(
                "spaces-to-tabs-leading",
                "Space to TAB (Leading)",
                MenuPlace::nested_in(Edit, 3, BLANK_SUBMENU, 1),
                Keys::NONE,
            ),
            // The MVP's stand-ins for the Column Editor (M6).
            Self::AddPrefixSuffix => editing(
                "add-prefix-suffix",
                "Add Prefix/Suffix…",
                MenuPlace::at(Edit, 4),
                Keys::NONE,
            ),
            Self::InsertNumbers => editing(
                "insert-numbers",
                "Insert Numbers…",
                MenuPlace::at(Edit, 4),
                Keys::NONE,
            ),
            // GtkSourceView's move-to-matching-bracket.
            Self::GoToMatchingBrace => editing(
                "go-to-matching-brace",
                "Go to Matching Brace",
                MenuPlace::at(Search, 2),
                Keys::app(&["<Control>b"]),
            ),
            Self::SelectToMatchingBrace => editing(
                "select-to-matching-brace",
                "Select to Matching Brace",
                MenuPlace::at(Search, 2),
                Keys::app(&["<Control><Alt>b"]),
            ),
            // Ctrl+F3 and Ctrl+Shift+F3 after an F-key-free key.
            Self::SelectAndFindNext => editing(
                "select-and-find-next",
                "Select and Find Next",
                MenuPlace::at(Search, 0),
                Keys::app(&["<Control><Alt>f", "<Control>F3"]),
            ),
            Self::SelectAndFindPrevious => editing(
                "select-and-find-previous",
                "Select and Find Previous",
                MenuPlace::at(Search, 0),
                Keys::app(&["<Control><Alt><Shift>f", "<Control><Shift>F3"]),
            ),
            Self::DocumentMap => toggle("document-map", "Document Map", MenuPlace::at(View, 0)),
            // The status bar's indentation item opens the same popover.
            Self::ChooseIndentation => {
                command("choose-indentation", "Indentation…", None, Keys::NONE)
            }
            Self::DetectIndentation => command(
                "detect-indentation",
                "Detect Indentation from Content",
                None,
                Keys::NONE,
            ),
            Self::OpenSettings => command(
                "open-settings",
                "Open Settings (config.toml)",
                MenuPlace::at(Settings, 0),
                Keys::NONE,
            ),
            Self::OpenKeyboardShortcuts => command(
                "open-keyboard-shortcuts",
                "Open Keyboard Shortcuts (keys.toml)",
                MenuPlace::at(Settings, 0),
                Keys::NONE,
            ),
            Self::SetAsDefaultEditor => command(
                "set-as-default-editor",
                "Set as Default Editor…",
                MenuPlace::at(Settings, 1),
                Keys::NONE,
            ),
            // The JSON and XML tools are on Ctrl+Alt+Shift, with J and X for the validations.
            Self::FormatJson => editing(
                "format-json",
                "Format JSON",
                MenuPlace::nested(Tools, JSON_SUBMENU, 0),
                Keys::app(&["<Control><Alt><Shift>m"]),
            ),
            Self::MinifyJson => editing(
                "minify-json",
                "Minify JSON",
                MenuPlace::nested(Tools, JSON_SUBMENU, 0),
                Keys::app(&["<Control><Alt><Shift>c"]),
            ),
            Self::ValidateJson => editing(
                "validate-json",
                "Validate JSON",
                MenuPlace::nested(Tools, JSON_SUBMENU, 0),
                Keys::app(&["<Control><Alt><Shift>j"]),
            ),
            Self::FormatXml => editing(
                "format-xml",
                "Format XML",
                MenuPlace::nested(Tools, XML_SUBMENU, 0),
                Keys::app(&["<Control><Alt><Shift>b"]),
            ),
            Self::ValidateXml => editing(
                "validate-xml",
                "Validate XML",
                MenuPlace::nested(Tools, XML_SUBMENU, 0),
                Keys::app(&["<Control><Alt><Shift>x"]),
            ),
            Self::KeyboardShortcuts => command(
                "keyboard-shortcuts",
                "Keyboard Shortcuts",
                MenuPlace::at(Help, 0),
                Keys::NONE,
            ),
            Self::About => command("about", "About Stet", MenuPlace::at(Help, 0), Keys::NONE),
            // Column selection on Alt+Shift takes over GtkSourceView's `move-viewport` bindings:
            // application accelerators run before the editor's own bindings (ADR-009, ADR-016).
            Self::ColumnSelectLeft => command(
                "column-select-left",
                "Column Select Left",
                None,
                Keys::app(&["<Alt><Shift>Left", "<Alt><Shift>KP_Left"]),
            ),
            Self::ColumnSelectRight => command(
                "column-select-right",
                "Column Select Right",
                None,
                Keys::app(&["<Alt><Shift>Right", "<Alt><Shift>KP_Right"]),
            ),
            Self::ColumnSelectUp => command(
                "column-select-up",
                "Column Select Up",
                None,
                Keys::app(&["<Alt><Shift>Up", "<Alt><Shift>KP_Up"]),
            ),
            Self::ColumnSelectDown => command(
                "column-select-down",
                "Column Select Down",
                None,
                Keys::app(&["<Alt><Shift>Down", "<Alt><Shift>KP_Down"]),
            ),
            Self::ColumnSelectLineStart => command(
                "column-select-line-start",
                "Column Select to Line Start",
                None,
                Keys::app(&["<Alt><Shift>Home", "<Alt><Shift>KP_Home"]),
            ),
            Self::ColumnSelectLineEnd => command(
                "column-select-line-end",
                "Column Select to Line End",
                None,
                Keys::app(&["<Alt><Shift>End", "<Alt><Shift>KP_End"]),
            ),
            Self::ColumnSelectPageUp => command(
                "column-select-page-up",
                "Column Select Page Up",
                None,
                Keys::app(&["<Alt><Shift>Page_Up", "<Alt><Shift>KP_Page_Up"]),
            ),
            Self::ColumnSelectPageDown => command(
                "column-select-page-down",
                "Column Select Page Down",
                None,
                Keys::app(&["<Alt><Shift>Page_Down", "<Alt><Shift>KP_Page_Down"]),
            ),
            Self::ColumnBeginEndSelect => command(
                "column-begin-end-select",
                "Begin/End Select in Column Mode",
                MenuPlace::at(Edit, 2),
                Keys::app(&["<Alt><Shift>b"]),
            ),
            Self::ColumnEditor => command(
                "column-editor",
                "Column Editor…",
                MenuPlace::at(Edit, 2),
                Keys::app(&["<Alt>c"]),
            ),
            // Search › Bookmark. Ctrl+F2, F2 and Shift+F2 come after the F-key-free keys of
            // VS Code's Bookmarks extension (ADR-009 amendment, M7).
            Self::ToggleBookmark => in_search(
                "toggle-bookmark",
                "Toggle Bookmark",
                BOOKMARK_SUBMENU,
                0,
                Keys::app(&["<Control><Alt>k", "<Control>F2"]),
            ),
            Self::NextBookmark => in_search(
                "next-bookmark",
                "Next Bookmark",
                BOOKMARK_SUBMENU,
                0,
                Keys::app(&["<Control><Alt>l", "F2"]),
            ),
            Self::PreviousBookmark => in_search(
                "previous-bookmark",
                "Previous Bookmark",
                BOOKMARK_SUBMENU,
                0,
                Keys::app(&["<Control><Alt>j", "<Shift>F2"]),
            ),
            Self::ClearBookmarks => in_search(
                "clear-bookmarks",
                "Clear All Bookmarks",
                BOOKMARK_SUBMENU,
                0,
                Keys::NONE,
            ),
            Self::CutBookmarkedLines => in_search(
                "cut-bookmarked-lines",
                "Cut Bookmarked Lines",
                BOOKMARK_SUBMENU,
                1,
                Keys::NONE,
            ),
            Self::CopyBookmarkedLines => in_search(
                "copy-bookmarked-lines",
                "Copy Bookmarked Lines",
                BOOKMARK_SUBMENU,
                1,
                Keys::NONE,
            ),
            Self::PasteToBookmarkedLines => in_search(
                "paste-to-bookmarked-lines",
                "Paste to (Replace) Bookmarked Lines",
                BOOKMARK_SUBMENU,
                1,
                Keys::NONE,
            ),
            Self::RemoveBookmarkedLines => in_search(
                "remove-bookmarked-lines",
                "Remove Bookmarked Lines",
                BOOKMARK_SUBMENU,
                1,
                Keys::NONE,
            ),
            Self::RemoveUnbookmarkedLines => in_search(
                "remove-unbookmarked-lines",
                "Remove Non-Bookmarked Lines",
                BOOKMARK_SUBMENU,
                1,
                Keys::NONE,
            ),
            Self::InverseBookmarks => in_search(
                "inverse-bookmarks",
                "Inverse Bookmarks",
                BOOKMARK_SUBMENU,
                1,
                Keys::NONE,
            ),
            // The find bar's Mark row (M7).
            Self::Mark => command(
                "mark",
                "Mark…",
                MenuPlace::at(Search, 3),
                Keys::app(&["<Control>m"]),
            ),
            Self::MarkAll => command("mark-all", "Mark All", None, Keys::NONE),
            Self::ClearMarks => command(
                "clear-marks",
                "Clear All Marks",
                MenuPlace::at(Search, 3),
                Keys::NONE,
            ),
            Self::CopyMarkedText => command(
                "copy-marked-text",
                "Copy Marked Text",
                MenuPlace::at(Search, 3),
                Keys::NONE,
            ),
            Self::StyleToken1 => in_search(
                "style-token-1",
                "Using 1st Style",
                STYLE_TOKEN_SUBMENU,
                0,
                Keys::NONE,
            ),
            Self::StyleToken2 => in_search(
                "style-token-2",
                "Using 2nd Style",
                STYLE_TOKEN_SUBMENU,
                0,
                Keys::NONE,
            ),
            Self::StyleToken3 => in_search(
                "style-token-3",
                "Using 3rd Style",
                STYLE_TOKEN_SUBMENU,
                0,
                Keys::NONE,
            ),
            Self::StyleToken4 => in_search(
                "style-token-4",
                "Using 4th Style",
                STYLE_TOKEN_SUBMENU,
                0,
                Keys::NONE,
            ),
            Self::StyleToken5 => in_search(
                "style-token-5",
                "Using 5th Style",
                STYLE_TOKEN_SUBMENU,
                0,
                Keys::NONE,
            ),
            Self::ClearStyle1 => in_search(
                "clear-style-1",
                "Clear 1st Style",
                CLEAR_STYLE_SUBMENU,
                0,
                Keys::NONE,
            ),
            Self::ClearStyle2 => in_search(
                "clear-style-2",
                "Clear 2nd Style",
                CLEAR_STYLE_SUBMENU,
                0,
                Keys::NONE,
            ),
            Self::ClearStyle3 => in_search(
                "clear-style-3",
                "Clear 3rd Style",
                CLEAR_STYLE_SUBMENU,
                0,
                Keys::NONE,
            ),
            Self::ClearStyle4 => in_search(
                "clear-style-4",
                "Clear 4th Style",
                CLEAR_STYLE_SUBMENU,
                0,
                Keys::NONE,
            ),
            Self::ClearStyle5 => in_search(
                "clear-style-5",
                "Clear 5th Style",
                CLEAR_STYLE_SUBMENU,
                0,
                Keys::NONE,
            ),
            Self::ClearAllStyles => in_search(
                "clear-all-styles",
                "Clear All Styles",
                CLEAR_STYLE_SUBMENU,
                1,
                Keys::NONE,
            ),
            // Ctrl+1–5 jump down and Ctrl+Alt+1–5 up: Ctrl+Shift+1–5 can't be matched on every
            // layout (Shift turns the digit into a symbol). Ctrl+0 is Reset Zoom, so the Find
            // Mark Style jumps are Alt+M and Alt+Shift+M (ADR-009 amendment, M7).
            Self::JumpUp1 => jump(
                "jump-up-1",
                "1st Style",
                JUMP_UP_SUBMENU,
                0,
                Keys::app(&["<Control><Alt>1"]),
            ),
            Self::JumpUp2 => jump(
                "jump-up-2",
                "2nd Style",
                JUMP_UP_SUBMENU,
                0,
                Keys::app(&["<Control><Alt>2"]),
            ),
            Self::JumpUp3 => jump(
                "jump-up-3",
                "3rd Style",
                JUMP_UP_SUBMENU,
                0,
                Keys::app(&["<Control><Alt>3"]),
            ),
            Self::JumpUp4 => jump(
                "jump-up-4",
                "4th Style",
                JUMP_UP_SUBMENU,
                0,
                Keys::app(&["<Control><Alt>4"]),
            ),
            Self::JumpUp5 => jump(
                "jump-up-5",
                "5th Style",
                JUMP_UP_SUBMENU,
                0,
                Keys::app(&["<Control><Alt>5"]),
            ),
            Self::JumpUpMark => jump(
                "jump-up-mark",
                "Find Mark Style",
                JUMP_UP_SUBMENU,
                1,
                Keys::app(&["<Alt><Shift>m"]),
            ),
            Self::JumpDown1 => jump(
                "jump-down-1",
                "1st Style",
                JUMP_DOWN_SUBMENU,
                0,
                Keys::app(&["<Control>1"]),
            ),
            Self::JumpDown2 => jump(
                "jump-down-2",
                "2nd Style",
                JUMP_DOWN_SUBMENU,
                0,
                Keys::app(&["<Control>2"]),
            ),
            Self::JumpDown3 => jump(
                "jump-down-3",
                "3rd Style",
                JUMP_DOWN_SUBMENU,
                0,
                Keys::app(&["<Control>3"]),
            ),
            Self::JumpDown4 => jump(
                "jump-down-4",
                "4th Style",
                JUMP_DOWN_SUBMENU,
                0,
                Keys::app(&["<Control>4"]),
            ),
            Self::JumpDown5 => jump(
                "jump-down-5",
                "5th Style",
                JUMP_DOWN_SUBMENU,
                0,
                Keys::app(&["<Control>5"]),
            ),
            Self::JumpDownMark => jump(
                "jump-down-mark",
                "Find Mark Style",
                JUMP_DOWN_SUBMENU,
                1,
                Keys::app(&["<Alt>m"]),
            ),
            // View › Move/Clone Current Document, Focus on Another View (F8, after an F-key-free
            // key) and the synchronised scrolling toggles.
            Self::MoveToOtherView => command(
                "move-to-other-view",
                "Move to Other View",
                MenuPlace::nested_in(View, 4, MOVE_CLONE_SUBMENU, 0),
                Keys::NONE,
            ),
            Self::CloneToOtherView => command(
                "clone-to-other-view",
                "Clone to Other View",
                MenuPlace::nested_in(View, 4, MOVE_CLONE_SUBMENU, 0),
                Keys::NONE,
            ),
            Self::SwitchView => command(
                "switch-view",
                "Focus on Another View",
                MenuPlace::at(View, 4),
                Keys::app(&["<Control><Alt>o", "F8"]),
            ),
            Self::SyncVerticalScrolling => toggle(
                "sync-vertical-scrolling",
                "Synchronise Vertical Scrolling",
                MenuPlace::at(View, 4),
            ),
            Self::SyncHorizontalScrolling => toggle(
                "sync-horizontal-scrolling",
                "Synchronise Horizontal Scrolling",
                MenuPlace::at(View, 4),
            ),
            Self::Compare => in_compare("compare", "Compare", 0, Keys::app(&["<Control><Alt>c"])),
            Self::CompareWithFile => {
                in_compare("compare-with-file", "Compare with File…", 0, Keys::NONE)
            }
            Self::CompareWithClipboard => in_compare(
                "compare-with-clipboard",
                "Compare with Clipboard",
                0,
                Keys::app(&["<Control><Alt>m"]),
            ),
            Self::CompareWithSaved => in_compare(
                "compare-with-saved",
                "Compare with Saved Version",
                0,
                Keys::app(&["<Control><Alt>d"]),
            ),
            Self::PreviousDifference => in_compare(
                "previous-difference",
                "Previous Difference",
                1,
                Keys::app(&["<Alt>Page_Up"]),
            ),
            Self::NextDifference => in_compare(
                "next-difference",
                "Next Difference",
                1,
                Keys::app(&["<Alt>Page_Down"]),
            ),
            Self::CompareIgnoreWhitespace => toggle(
                "compare-ignore-whitespace",
                "Ignore Whitespace",
                MenuPlace::nested(Tools, COMPARE_SUBMENU, 2),
            ),
            Self::CompareIgnoreCase => toggle(
                "compare-ignore-case",
                "Ignore Case",
                MenuPlace::nested(Tools, COMPARE_SUBMENU, 2),
            ),
            Self::ClearCompare => in_compare(
                "clear-compare",
                "Clear Compare",
                3,
                Keys::app(&["<Control><Alt>x"]),
            ),
        }
    }

    pub const fn name(self) -> &'static str {
        self.spec().name
    }

    pub const fn scope(self) -> ActionScope {
        self.spec().scope
    }

    pub const fn label(self) -> &'static str {
        self.spec().label
    }

    pub const fn kind(self) -> ActionKind {
        self.spec().kind
    }

    pub const fn keys(self) -> Keys {
        self.spec().keys
    }

    pub const fn key_scope(self) -> KeyScope {
        self.spec().key_scope
    }

    /// The default application accelerators, installed with `set_accels_for_action`; an
    /// editor-scoped action has none (its keys are the editors' own shortcuts).
    pub const fn default_accels(self) -> &'static [&'static str] {
        match self.spec().key_scope {
            KeyScope::Window => self.spec().keys.app,
            KeyScope::Editor => &[],
        }
    }

    /// Whether the keys are GtkTextView's own bindings, which Stet shows but cannot change
    /// (undo, the clipboard, select all).
    pub const fn has_widget_keys(self) -> bool {
        !self.spec().keys.widget.is_empty()
    }

    /// The name GTK uses to activate the action: `app.quit`, `win.save`.
    pub fn detailed_name(self) -> String {
        format!("{}.{}", self.scope().prefix(), self.name())
    }

    pub fn from_name(name: &str) -> Option<Self> {
        let name = name
            .strip_prefix("win.")
            .or_else(|| name.strip_prefix("app."))
            .unwrap_or(name);
        Self::ALL.into_iter().find(|id| id.name() == name)
    }

    /// `File › Save`, or `File › Open Recent › …` for nested entries.
    pub fn menu_path(self) -> Option<String> {
        let place = self.spec().menu?;
        Some(match place.submenu {
            Some(submenu) => format!("{} › {submenu}", place.menu.label()),
            None => place.menu.label().to_owned(),
        })
    }
}

/// An accelerator string split into modifiers and key name, as GTK spells them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Accelerator {
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
    pub super_key: bool,
    /// The key name, lowercased for letters (GTK lowercases them too).
    pub key: String,
}

impl Accelerator {
    /// Parses `<Control><Shift>z`-style strings. Accepts GTK's modifier spellings; rejects
    /// empty keys and unknown modifiers. Key names are checked by [`is_key_name`].
    pub fn parse(accel: &str) -> Option<Self> {
        let mut rest = accel;
        let mut parsed = Self {
            control: false,
            shift: false,
            alt: false,
            super_key: false,
            key: String::new(),
        };
        while let Some(tail) = rest.strip_prefix('<') {
            let (modifier, after) = tail.split_once('>')?;
            match modifier.to_ascii_lowercase().as_str() {
                "control" | "ctrl" | "ctl" | "primary" => parsed.control = true,
                "shift" | "shft" => parsed.shift = true,
                "alt" | "mod1" => parsed.alt = true,
                "super" => parsed.super_key = true,
                _ => return None,
            }
            rest = after;
        }
        if !is_key_name(rest) {
            return None;
        }
        parsed.key = if rest.chars().count() == 1 {
            rest.to_ascii_lowercase()
        } else {
            rest.to_owned()
        };
        Some(parsed)
    }

    /// F1–F35: the keys the user's F-row sends as media keys unless Fn is held.
    pub fn is_function_key(&self) -> bool {
        self.key
            .strip_prefix('F')
            .and_then(|number| number.parse::<u8>().ok())
            .is_some_and(|number| (1..=35).contains(&number))
    }

    /// One spelling per key, `<Control><Shift><Alt><Super>key`, to compare keys written
    /// differently (`<Ctrl>D`, `<Control>d`).
    pub fn canonical(&self) -> String {
        let mut out = String::new();
        for (on, name) in [
            (self.control, "<Control>"),
            (self.shift, "<Shift>"),
            (self.alt, "<Alt>"),
            (self.super_key, "<Super>"),
        ] {
            if on {
                out.push_str(name);
            }
        }
        out.push_str(&self.key);
        out
    }

    /// `Ctrl+Shift+Z`, for menus and the palette.
    pub fn display(&self) -> String {
        let mut out = String::new();
        for (on, name) in [
            (self.super_key, "Super+"),
            (self.control, "Ctrl+"),
            (self.alt, "Alt+"),
            (self.shift, "Shift+"),
        ] {
            if on {
                out.push_str(name);
            }
        }
        out.push_str(&display_key(&self.key));
        out
    }
}

fn display_key(key: &str) -> String {
    match key {
        "plus" => "+".to_owned(),
        "minus" => "-".to_owned(),
        "equal" => "=".to_owned(),
        "Page_Up" => "PgUp".to_owned(),
        "Page_Down" => "PgDn".to_owned(),
        "Return" => "Enter".to_owned(),
        "Up" => "↑".to_owned(),
        "Down" => "↓".to_owned(),
        "Left" => "←".to_owned(),
        "Right" => "→".to_owned(),
        "slash" => "/".to_owned(),
        "backslash" => "\\".to_owned(),
        "Delete" => "Del".to_owned(),
        "Insert" => "Ins".to_owned(),
        _ => match key.strip_prefix("KP_") {
            Some("Add") => "Num +".to_owned(),
            Some("Subtract") => "Num -".to_owned(),
            Some("Divide") => "Num /".to_owned(),
            Some(other) => format!("Num {}", display_key(other)),
            None if key.chars().count() == 1 => key.to_ascii_uppercase(),
            None => key.to_owned(),
        },
    }
}

/// Key names the registry may use: letters, digits, F-keys and a fixed list of GDK key names.
/// The app's tests also run every accelerator through GTK's own parser.
pub fn is_key_name(key: &str) -> bool {
    const NAMED: &[&str] = &[
        "Return",
        "Tab",
        "Escape",
        "Delete",
        "Insert",
        "BackSpace",
        "Home",
        "End",
        "Page_Up",
        "Page_Down",
        "Up",
        "Down",
        "Left",
        "Right",
        "space",
        "plus",
        "minus",
        "equal",
        "period",
        "comma",
        "slash",
        "backslash",
        "semicolon",
        "apostrophe",
        "grave",
        "bracketleft",
        "bracketright",
        "KP_Add",
        "KP_Subtract",
        "KP_Divide",
        "KP_Multiply",
        "KP_Enter",
        "KP_Up",
        "KP_Down",
        "KP_Left",
        "KP_Right",
        "KP_Page_Up",
        "KP_Next",
        "KP_Page_Down",
        "KP_Home",
        "KP_End",
    ];
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => c.is_ascii_alphanumeric(),
        _ => {
            NAMED.contains(&key)
                || key
                    .strip_prefix('F')
                    .and_then(|number| number.parse::<u8>().ok())
                    .is_some_and(|number| (1..=35).contains(&number))
        }
    }
}

/// How a punctuation key is typed on a layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Base,
    Shift,
    AltGr,
}

/// The punctuation keys the keymap audit covers, on the US and the Norwegian (`no`) layout
/// (from xkeyboard-config's `us` and `no(basic)`). Letters, digits, the keypad, arrows and
/// F-keys are the same on both.
pub const PUNCTUATION_LEVELS: &[(&str, Level, Level)] = &[
    ("plus", Level::Shift, Level::Base),
    ("equal", Level::Base, Level::Shift),
    ("minus", Level::Base, Level::Base),
    ("period", Level::Base, Level::Base),
    ("comma", Level::Base, Level::Base),
    ("slash", Level::Base, Level::Shift),
    ("backslash", Level::Base, Level::Base),
    ("semicolon", Level::Base, Level::Shift),
    ("apostrophe", Level::Base, Level::Base),
    ("grave", Level::Base, Level::Shift),
    ("bracketleft", Level::Base, Level::AltGr),
    ("bracketright", Level::Base, Level::AltGr),
];

/// The US and Norwegian levels of `key`, or `None` when the key is layout-independent here.
pub fn punctuation_levels(key: &str) -> Option<(Level, Level)> {
    PUNCTUATION_LEVELS
        .iter()
        .find(|(name, _, _)| *name == key)
        .map(|&(_, us, no)| (us, no))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    fn every_key() -> impl Iterator<Item = (ActionId, &'static str)> {
        ActionId::ALL
            .into_iter()
            .flat_map(|id| id.keys().all().map(move |key| (id, key)))
    }

    #[test]
    fn all_lists_every_action_once_in_declaration_order() {
        let names: HashSet<_> = ActionId::ALL.iter().map(|id| id.name()).collect();
        assert_eq!(names.len(), ActionId::ALL.len());
        for id in ActionId::ALL {
            assert_eq!(ActionId::from_name(id.name()), Some(id));
            assert_eq!(ActionId::from_name(&id.detailed_name()), Some(id));
        }
        assert_eq!(ActionId::from_name("win.no-such-action"), None);
    }

    #[test]
    fn names_are_kebab_case_gaction_names() {
        for id in ActionId::ALL {
            let name = id.name();
            assert!(!name.starts_with('-') && !name.ends_with('-'), "{name}");
            assert!(!name.contains("--"), "{name}");
            assert!(name.starts_with(|c: char| c.is_ascii_lowercase()), "{name}");
            // Digits for encoding names such as `encode-utf8`; GAction names allow them.
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'),
                "{name}"
            );
        }
    }

    #[test]
    fn labels_are_present_and_unique_per_menu() {
        let mut seen = HashSet::new();
        for id in ActionId::ALL {
            assert!(!id.label().trim().is_empty(), "{id:?}");
            if let Some(place) = id.spec().menu {
                assert!(
                    seen.insert((place.menu, place.submenu, id.label())),
                    "{id:?}"
                );
            }
        }
    }

    #[test]
    fn no_key_is_used_twice() {
        let mut seen: HashMap<Accelerator, ActionId> = HashMap::new();
        for (id, key) in every_key() {
            let accel = Accelerator::parse(key).unwrap_or_else(|| panic!("{id:?}: {key}"));
            if let Some(other) = seen.insert(accel, id) {
                panic!("{key} is bound to both {other:?} and {id:?}");
            }
        }
    }

    #[test]
    fn every_key_is_a_well_formed_accelerator() {
        for (id, key) in every_key() {
            assert!(Accelerator::parse(key).is_some(), "{id:?}: {key}");
        }
        for bad in [
            "",
            "<Control>",
            "<Ctrl",
            "<Hyper>a",
            "<Control>ab",
            "<Control>F0",
        ] {
            assert_eq!(Accelerator::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn no_action_is_reachable_only_through_an_f_key() {
        for id in ActionId::ALL {
            let keys: Vec<Accelerator> = id
                .keys()
                .all()
                .map(|key| Accelerator::parse(key).unwrap())
                .collect();
            if keys.iter().any(Accelerator::is_function_key) {
                assert!(
                    keys.iter().any(|key| !key.is_function_key()),
                    "{id:?} has only F-key bindings"
                );
                let first = &keys[0];
                assert!(
                    !first.is_function_key(),
                    "{id:?}: menus show the first key, so it must not be an F-key"
                );
            }
        }
    }

    #[test]
    fn punctuation_keys_work_on_the_us_and_norwegian_layouts() {
        for id in ActionId::ALL {
            let keys: Vec<Accelerator> = id
                .keys()
                .all()
                .map(|key| Accelerator::parse(key).unwrap())
                .collect();
            let levels: Vec<(Level, Level)> = keys
                .iter()
                .filter_map(|key| punctuation_levels(&key.key))
                .collect();
            for (us, no) in &levels {
                assert_ne!(*us, Level::AltGr, "{id:?} needs AltGr on the US layout");
                assert_ne!(
                    *no,
                    Level::AltGr,
                    "{id:?} needs AltGr on the Norwegian layout"
                );
            }
            if !levels.is_empty() {
                assert!(
                    levels.iter().any(|(us, _)| *us == Level::Base),
                    "{id:?} has no unshifted key on the US layout"
                );
                assert!(
                    levels.iter().any(|(_, no)| *no == Level::Base),
                    "{id:?} has no unshifted key on the Norwegian layout"
                );
            }
        }
    }

    #[test]
    fn keys_avoid_known_grabs() {
        // fcitx5 (Ctrl+Space trigger, Unicode addon, toggle preedit), VT switching and
        // Omarchy's non-SUPER Hyprland binds never reach the app.
        let grabbed = [
            "<Control>space",
            "<Control><Shift>u",
            "<Control><Alt><Shift>u",
            "<Control><Alt>p",
            "<Control><Alt>Delete",
            "<Alt>Tab",
            "<Alt><Shift>Tab",
            "<Control><Alt>Tab",
        ]
        .map(|key| Accelerator::parse(key).unwrap());
        for (id, key) in every_key() {
            let accel = Accelerator::parse(key).unwrap();
            assert!(!grabbed.contains(&accel), "{id:?}: {key} is grabbed");
            assert!(
                !(accel.control && accel.alt && accel.is_function_key()),
                "{id:?}: Ctrl+Alt+F-keys switch virtual terminals"
            );
            assert!(!accel.super_key, "{id:?}: SUPER belongs to Hyprland");
        }
    }

    #[test]
    fn the_default_preset() {
        let keys = |id: ActionId| id.keys().all().collect::<Vec<_>>();
        assert_eq!(keys(ActionId::NewTab), ["<Control>n"]);
        assert_eq!(keys(ActionId::SaveAs), ["<Control><Alt>s"]);
        assert_eq!(keys(ActionId::SaveAll), ["<Control><Shift>s"]);
        assert_eq!(keys(ActionId::CloseAll), ["<Control><Shift>w"]);
        assert_eq!(keys(ActionId::RestoreClosedTab), ["<Control><Shift>t"]);
        assert_eq!(keys(ActionId::GoToLine), ["<Control>g"]);
        assert!(keys(ActionId::Quit).contains(&"<Alt>F4"));
        assert!(keys(ActionId::FindNext).contains(&"F3"));
        assert!(keys(ActionId::FindPrevious).contains(&"<Shift>F3"));
        assert!(keys(ActionId::FullScreen).contains(&"F11"));
        assert!(keys(ActionId::CommandPalette).contains(&"F1"));
        assert_eq!(keys(ActionId::Replace), ["<Control>h"]);
        assert_eq!(keys(ActionId::FindInFiles), ["<Control><Shift>f"]);
        assert!(keys(ActionId::SearchResults).contains(&"F7"));
        assert!(keys(ActionId::NextSearchResult).contains(&"F4"));
        assert!(keys(ActionId::PreviousSearchResult).contains(&"<Shift>F4"));
        assert!(keys(ActionId::Redo).contains(&"<Control>y"));
        // M5's text tools.
        assert_eq!(keys(ActionId::DuplicateLine), ["<Control>d"]);
        assert_eq!(keys(ActionId::CutLine), ["<Control>l"]);
        assert_eq!(keys(ActionId::CopyLine), ["<Control><Shift>x"]);
        assert_eq!(keys(ActionId::DeleteLine), ["<Control><Shift>l"]);
        assert_eq!(keys(ActionId::TransposeLine), ["<Control>t"]);
        assert_eq!(keys(ActionId::MoveLineUp), ["<Control><Shift>Up"]);
        assert_eq!(keys(ActionId::MoveLineDown), ["<Control><Shift>Down"]);
        assert_eq!(keys(ActionId::JoinLines), ["<Control>j"]);
        assert_eq!(keys(ActionId::BlankLineAbove), ["<Control><Alt>Return"]);
        assert_eq!(
            keys(ActionId::BlankLineBelow),
            ["<Control><Alt><Shift>Return"]
        );
        assert_eq!(keys(ActionId::Lowercase), ["<Control>u"]);
        assert_eq!(keys(ActionId::ProperCase), ["<Alt>u"]);
        assert_eq!(keys(ActionId::SentenceCase), ["<Control><Alt>u"]);
        assert_eq!(keys(ActionId::ToggleComment), ["<Control>q"]);
        assert_eq!(keys(ActionId::CommentLines), ["<Control>k"]);
        assert_eq!(keys(ActionId::UncommentLines), ["<Control><Shift>k"]);
        assert_eq!(keys(ActionId::ToggleBlockComment), ["<Control><Shift>q"]);
        assert_eq!(keys(ActionId::GoToMatchingBrace), ["<Control>b"]);
        assert_eq!(keys(ActionId::SelectToMatchingBrace), ["<Control><Alt>b"]);
        assert!(keys(ActionId::SelectAndFindNext).contains(&"<Control>F3"));
        assert!(keys(ActionId::SelectAndFindPrevious).contains(&"<Control><Shift>F3"));
        // fcitx5 takes Ctrl+Shift+U (ADR-009 amendment of M5).
        assert_eq!(keys(ActionId::Uppercase), ["<Alt><Shift>u"]);
        assert_eq!(keys(ActionId::ColumnEditor), ["<Alt>c"]);
        assert_eq!(keys(ActionId::ColumnBeginEndSelect), ["<Alt><Shift>b"]);
        for (id, key) in [
            (ActionId::ColumnSelectLeft, "<Alt><Shift>Left"),
            (ActionId::ColumnSelectRight, "<Alt><Shift>Right"),
            (ActionId::ColumnSelectUp, "<Alt><Shift>Up"),
            (ActionId::ColumnSelectDown, "<Alt><Shift>Down"),
            (ActionId::ColumnSelectLineStart, "<Alt><Shift>Home"),
            (ActionId::ColumnSelectLineEnd, "<Alt><Shift>End"),
            (ActionId::ColumnSelectPageUp, "<Alt><Shift>Page_Up"),
            (ActionId::ColumnSelectPageDown, "<Alt><Shift>Page_Down"),
        ] {
            assert_eq!(keys(id).first(), Some(&key), "{id:?}");
        }
        assert_eq!(ActionId::Quit.scope(), ActionScope::App);
        assert_eq!(ActionId::CloseTab.scope(), ActionScope::Window);
        assert_eq!(ActionId::Quit.detailed_name(), "app.quit");
        assert_eq!(ActionId::NewTab.detailed_name(), "win.new-tab");
    }

    #[test]
    fn navigation_keys_m8() {
        let keys = |id: ActionId| id.keys().all().collect::<Vec<_>>();
        assert_eq!(keys(ActionId::QuickOpen), ["<Control>p"]);
        assert_eq!(keys(ActionId::NextRecentTab), ["<Control>Tab"]);
        assert_eq!(keys(ActionId::PreviousRecentTab), ["<Control><Shift>Tab"]);
        assert_eq!(keys(ActionId::NextTab), ["<Control>Page_Down"]);
        assert_eq!(keys(ActionId::PreviousTab), ["<Control>Page_Up"]);
        assert_eq!(keys(ActionId::GoBack), ["<Alt>Left"]);
        assert_eq!(keys(ActionId::GoForward), ["<Alt>Right"]);
        for id in [
            ActionId::PinTab,
            ActionId::UnpinTab,
            ActionId::ReplaceInFiles,
        ] {
            assert!(id.keys().is_empty(), "{id:?}");
            assert!(id.spec().palette, "{id:?}");
        }
        assert_eq!(ActionId::GoBack.menu_path().as_deref(), Some("Search"));
        assert_eq!(ActionId::PinTab.menu_path().as_deref(), Some("View"));
        assert_eq!(ActionId::ReplaceInFiles.menu_path(), None);
    }

    #[test]
    fn marking_view_and_compare_keys_m7() {
        let keys = |id: ActionId| id.keys().all().collect::<Vec<_>>();
        assert_eq!(
            keys(ActionId::ToggleBookmark),
            ["<Control><Alt>k", "<Control>F2"]
        );
        assert_eq!(keys(ActionId::NextBookmark), ["<Control><Alt>l", "F2"]);
        assert_eq!(
            keys(ActionId::PreviousBookmark),
            ["<Control><Alt>j", "<Shift>F2"]
        );
        assert_eq!(keys(ActionId::Mark), ["<Control>m"]);
        for (number, (down, up)) in [
            (ActionId::JumpDown1, ActionId::JumpUp1),
            (ActionId::JumpDown2, ActionId::JumpUp2),
            (ActionId::JumpDown3, ActionId::JumpUp3),
            (ActionId::JumpDown4, ActionId::JumpUp4),
            (ActionId::JumpDown5, ActionId::JumpUp5),
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(keys(down), [format!("<Control>{}", number + 1)]);
            assert_eq!(keys(up), [format!("<Control><Alt>{}", number + 1)]);
        }
        assert_eq!(keys(ActionId::JumpDownMark), ["<Alt>m"]);
        assert_eq!(keys(ActionId::JumpUpMark), ["<Alt><Shift>m"]);
        for id in [
            ActionId::JumpDown1,
            ActionId::JumpUp5,
            ActionId::JumpDownMark,
        ] {
            assert_eq!(id.key_scope(), KeyScope::Editor, "{id:?}");
        }
        assert_eq!(ActionId::ToggleBookmark.key_scope(), KeyScope::Window);
        // Ctrl+0 is Reset Zoom, not a jump.
        assert!(keys(ActionId::ZoomReset).contains(&"<Control>0"));
        assert_eq!(keys(ActionId::SwitchView), ["<Control><Alt>o", "F8"]);
        assert_eq!(keys(ActionId::Compare), ["<Control><Alt>c"]);
        assert_eq!(keys(ActionId::CompareWithClipboard), ["<Control><Alt>m"]);
        assert_eq!(keys(ActionId::CompareWithSaved), ["<Control><Alt>d"]);
        assert_eq!(keys(ActionId::ClearCompare), ["<Control><Alt>x"]);
        assert_eq!(keys(ActionId::NextDifference), ["<Alt>Page_Down"]);
        assert_eq!(keys(ActionId::PreviousDifference), ["<Alt>Page_Up"]);
        for id in [
            ActionId::MoveToOtherView,
            ActionId::CloneToOtherView,
            ActionId::CompareWithFile,
            ActionId::RemoveUnbookmarkedLines,
        ] {
            assert!(id.keys().is_empty(), "{id:?}");
            assert!(id.spec().palette, "{id:?}");
        }
        assert_eq!(
            ActionId::ToggleBookmark.menu_path().as_deref(),
            Some("Search › Bookmark")
        );
        assert_eq!(
            ActionId::JumpDownMark.menu_path().as_deref(),
            Some("Search › Jump Down")
        );
        assert_eq!(
            ActionId::CloneToOtherView.menu_path().as_deref(),
            Some("View › Move/Clone Current Document")
        );
        assert_eq!(
            ActionId::NextDifference.menu_path().as_deref(),
            Some("Tools › Compare")
        );
        assert_eq!(ActionId::MarkAll.menu_path(), None);
        assert_eq!(ActionId::CompareIgnoreCase.kind(), ActionKind::Toggle);
        assert_eq!(ActionId::SyncVerticalScrolling.kind(), ActionKind::Toggle);
    }

    #[test]
    fn digits_are_never_keys_with_shift() {
        // Shift turns a digit key into a symbol that differs between layouts (`!`, `"` or `@`,
        // `#`, `¤` or `$`, `%`), so GTK can't match `<Control><Shift>1` everywhere.
        for (id, key) in every_key() {
            let accel = Accelerator::parse(key).unwrap();
            let digit = accel.key.len() == 1 && accel.key.chars().all(|c| c.is_ascii_digit());
            assert!(!(digit && accel.shift), "{id:?}: {key}");
        }
    }

    #[test]
    fn widget_keys_are_never_installed_as_accelerators() {
        for id in [
            ActionId::Undo,
            ActionId::Redo,
            ActionId::Cut,
            ActionId::Copy,
            ActionId::Paste,
            ActionId::Delete,
            ActionId::SelectAll,
        ] {
            assert!(id.default_accels().is_empty(), "{id:?}");
            assert!(!id.keys().widget.is_empty(), "{id:?}");
        }
    }

    #[test]
    fn the_encoding_menu_keeps_its_order() {
        let labels = |place: Option<MenuPlace>| {
            ActionId::ALL
                .into_iter()
                .filter(|id| id.spec().menu == place)
                .map(ActionId::label)
                .collect::<Vec<_>>()
        };
        assert_eq!(Menu::ALL[4], Menu::Encoding);
        assert_eq!(
            labels(MenuPlace::at(Menu::Encoding, 0)),
            [
                "Encode in UTF-8",
                "Encode in UTF-8-BOM",
                "Encode in UTF-16 BE BOM",
                "Encode in UTF-16 LE BOM"
            ]
        );
        assert_eq!(
            labels(MenuPlace::at(Menu::Encoding, 1)),
            [
                "Convert to ANSI (Windows-1252)",
                "Convert to UTF-8",
                "Convert to UTF-8-BOM",
                "Convert to UTF-16 BE BOM",
                "Convert to UTF-16 LE BOM"
            ]
        );
        assert_eq!(
            labels(MenuPlace::nested_in(Menu::Edit, 3, EOL_SUBMENU, 0)),
            ["Windows (CR LF)", "Unix (LF)", "Macintosh (CR)"]
        );
        assert_eq!(
            ActionId::Reinterpret.menu_path().as_deref(),
            Some("Encoding › Character Sets")
        );
        assert_eq!(ActionId::Reinterpret.kind(), ActionKind::WithString);
        assert_eq!(ActionId::ConvertEncoding.kind(), ActionKind::WithString);
    }

    #[test]
    fn menu_and_palette_rules() {
        for id in ActionId::ALL {
            let spec = id.spec();
            if spec.kind == ActionKind::WithString {
                assert!(!spec.palette, "{id:?} is reached through its own entries");
            }
            assert!(
                spec.menu.is_some() || spec.palette || spec.kind == ActionKind::WithString,
                "{id:?} is unreachable"
            );
        }
        assert_eq!(ActionId::Save.menu_path().as_deref(), Some("File"));
        assert_eq!(
            ActionId::ClearRecent.menu_path().as_deref(),
            Some("File › Open Recent")
        );
        assert_eq!(ActionId::SetLanguage.menu_path(), None);
    }

    #[test]
    fn text_tools_have_editor_keys_and_no_accelerators() {
        for id in ActionId::ALL {
            match id.key_scope() {
                KeyScope::Editor => {
                    assert!(id.default_accels().is_empty(), "{id:?}");
                    assert!(id.keys().widget.is_empty(), "{id:?}");
                    assert_eq!(id.scope(), ActionScope::Window, "{id:?}");
                }
                KeyScope::Window => assert_eq!(id.default_accels(), id.keys().app, "{id:?}"),
            }
        }
        for id in [
            ActionId::DuplicateLine,
            ActionId::Uppercase,
            ActionId::ToggleComment,
            ActionId::GoToMatchingBrace,
            ActionId::FormatJson,
            ActionId::SortIntegerAscending,
        ] {
            assert_eq!(id.key_scope(), KeyScope::Editor, "{id:?}");
        }
        for id in [ActionId::Find, ActionId::RenameFile, ActionId::DocumentMap] {
            assert_eq!(id.key_scope(), KeyScope::Window, "{id:?}");
        }
    }

    #[test]
    fn overridden_builtins_are_well_formed_and_unique() {
        let mut seen = HashSet::new();
        for key in OVERRIDDEN_BUILTINS {
            let accel = Accelerator::parse(key).unwrap_or_else(|| panic!("{key}"));
            assert!(seen.insert(accel.canonical()), "{key} twice");
        }
        // Copy Current Line takes Ctrl+Shift+X from GtkSourceView's change-number.
        let copy_line = Accelerator::parse(ActionId::CopyLine.keys().app[0]).unwrap();
        assert!(seen.contains(&copy_line.canonical()));
    }

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

    #[test]
    fn canonical_spellings_compare_keys() {
        let canonical = |key: &str| Accelerator::parse(key).unwrap().canonical();
        assert_eq!(canonical("<Ctrl>D"), "<Control>d");
        assert_eq!(canonical("<Shift><Primary>x"), "<Control><Shift>x");
        assert_eq!(canonical("<Alt><Shift>KP_Up"), "<Shift><Alt>KP_Up");
        assert_eq!(canonical("F3"), "F3");
    }

    #[test]
    fn accelerators_display_readably() {
        let display = |key: &str| Accelerator::parse(key).unwrap().display();
        assert_eq!(display("<Control><Shift>z"), "Ctrl+Shift+Z");
        assert_eq!(display("<Control><Alt>s"), "Ctrl+Alt+S");
        assert_eq!(display("<Alt>Down"), "Alt+↓");
        assert_eq!(display("<Alt><Shift>Left"), "Alt+Shift+←");
        assert_eq!(display("<Alt><Shift>KP_Page_Up"), "Alt+Shift+Num PgUp");
        assert_eq!(display("<Control>plus"), "Ctrl++");
        assert_eq!(display("<Control>KP_Add"), "Ctrl+Num +");
        assert_eq!(display("<Control>Page_Down"), "Ctrl+PgDn");
        assert_eq!(display("<Alt>Return"), "Alt+Enter");
        assert_eq!(display("F11"), "F11");
        assert_eq!(
            Accelerator::parse("<Control>Z"),
            Accelerator::parse("<ctrl>z")
        );
    }
}
