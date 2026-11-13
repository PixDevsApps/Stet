//! The action table: each action declared once, with its spec. The table's order is
//! [`ActionId::ALL`]'s: menus list their entries in it, and the palette starts from it.

use super::Menu::{Edit, Encoding, File, Help, Language, Main, Search, Settings, Tools, View};
use super::menu::{
    BLANK_SUBMENU, BOOKMARK_SUBMENU, CASE_SUBMENU, CHARACTER_SETS_SUBMENU, CLEAR_STYLE_SUBMENU,
    COMMENT_SUBMENU, COMPARE_SUBMENU, COPY_SUBMENU, EOL_SUBMENU, JSON_SUBMENU, JUMP_DOWN_SUBMENU,
    JUMP_UP_SUBMENU, LINES_SUBMENU, MOVE_CLONE_SUBMENU, RECENT_SUBMENU, STYLE_TOKEN_SUBMENU,
    XML_SUBMENU,
};
use super::{
    ActionKind, ActionScope, ActionSpec, KeyScope, Keys, MenuPlace, command, editing, in_compare,
    in_search, jump, sort, toggle, with_encoding,
};

/// Generates [`ActionId`], [`ActionId::ALL`] and [`ActionId::spec`] from one table of
/// `Variant => spec` rows; attributes and doc comments on a row go to its variant.
macro_rules! actions {
    ($($(#[$attr:meta])* $id:ident => $spec:expr,)*) => {
        /// Every user-visible command.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum ActionId {
            $($(#[$attr])* $id,)*
        }

        impl ActionId {
            /// Every action, in the table's order.
            pub const ALL: [Self; [$(Self::$id),*].len()] = [$(Self::$id),*];

            /// Everything the registry knows about the action.
            pub const fn spec(self) -> ActionSpec {
                match self {
                    $(Self::$id => $spec,)*
                }
            }
        }
    };
}

impl ActionId {
    /// The GAction name, without the `app.` or `win.` prefix.
    pub const fn name(self) -> &'static str {
        self.spec().name
    }

    /// Whether the action is the application's or the window's.
    pub const fn scope(self) -> ActionScope {
        self.spec().scope
    }

    /// The label menus and the palette show.
    pub const fn label(self) -> &'static str {
        self.spec().label
    }

    /// The shape of the GAction.
    pub const fn kind(self) -> ActionKind {
        self.spec().kind
    }

    /// The default keys.
    pub const fn keys(self) -> Keys {
        self.spec().keys
    }

    /// Where the keys work.
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

    /// The action called `name`, with or without its `app.` or `win.` prefix.
    pub fn from_name(name: &str) -> Option<Self> {
        let name = name
            .strip_prefix("win.")
            .or_else(|| name.strip_prefix("app."))
            .unwrap_or(name);
        Self::ALL.into_iter().find(|id| id.name() == name)
    }

    /// The menu that holds the action, `File`, or `File › Open Recent` for nested entries.
    pub fn menu_path(self) -> Option<String> {
        let place = self.spec().menu?;
        Some(match place.submenu {
            Some(submenu) => format!("{} › {submenu}", place.menu.label()),
            None => place.menu.label().to_owned(),
        })
    }
}

actions! {
    NewTab => command(
        "new-tab",
        "New",
        MenuPlace::at(File, 0),
        Keys::app(&["<Control>n"]),
    ),
    Open => command(
        "open",
        "Open…",
        MenuPlace::at(File, 0),
        Keys::app(&["<Control>o"]),
    ),
    // Ctrl+P is quick open, as in VS Code and Sublime Text, not Print, which Stet doesn't
    // have (ADR-009 amendment on navigation keys).
    QuickOpen => command(
        "quick-open",
        "Quick Open…",
        MenuPlace::at(File, 0),
        Keys::app(&["<Control>p"]),
    ),
    OpenRecent => ActionSpec {
        kind: ActionKind::WithString,
        palette: false,
        ..command(
            "open-recent",
            "Open Recent",
            MenuPlace::nested(File, RECENT_SUBMENU, 0),
            Keys::NONE,
        )
    },
    ClearRecent => command(
        "clear-recent",
        "Clear Recent Files",
        MenuPlace::nested(File, RECENT_SUBMENU, 1),
        Keys::NONE,
    ),
    Save => command(
        "save",
        "Save",
        MenuPlace::at(File, 1),
        Keys::app(&["<Control>s"]),
    ),
    SaveAs => command(
        "save-as",
        "Save As…",
        MenuPlace::at(File, 1),
        Keys::app(&["<Control><Alt>s"]),
    ),
    SaveAll => command(
        "save-all",
        "Save All",
        MenuPlace::at(File, 1),
        Keys::app(&["<Control><Shift>s"]),
    ),
    CloseTab => command(
        "close-tab",
        "Close",
        MenuPlace::at(File, 3),
        Keys::app(&["<Control>w"]),
    ),
    CloseAll => command(
        "close-all",
        "Close All",
        MenuPlace::at(File, 3),
        Keys::app(&["<Control><Shift>w"]),
    ),
    RestoreClosedTab => command(
        "restore-closed-tab",
        "Restore Closed Tab",
        MenuPlace::at(File, 3),
        Keys::app(&["<Control><Shift>t"]),
    ),
    // Discards every unsaved draft and its backup, after a confirmation (ADR-006).
    ForgetDrafts => command(
        "forget-drafts",
        "Forget Unsaved Drafts…",
        MenuPlace::at(File, 3),
        Keys::NONE,
    ),
    // Ctrl+Q stays free for Toggle Single Line Comment.
    Quit => ActionSpec {
        scope: ActionScope::App,
        ..command(
            "quit",
            "Quit",
            MenuPlace::at(File, 4),
            Keys::app(&["<Control><Alt>q", "<Alt>F4"]),
        )
    },
    Undo => command(
        "undo",
        "Undo",
        MenuPlace::at(Edit, 0),
        Keys::widget(&["<Control>z"]),
    ),
    Redo => command(
        "redo",
        "Redo",
        MenuPlace::at(Edit, 0),
        Keys::widget(&["<Control>y", "<Control><Shift>z"]),
    ),
    Cut => command(
        "cut",
        "Cut",
        MenuPlace::at(Edit, 1),
        Keys::widget(&["<Control>x", "<Shift>Delete"]),
    ),
    Copy => command(
        "copy",
        "Copy",
        MenuPlace::at(Edit, 1),
        Keys::widget(&["<Control>c", "<Control>Insert"]),
    ),
    Paste => command(
        "paste",
        "Paste",
        MenuPlace::at(Edit, 1),
        Keys::widget(&["<Control>v", "<Shift>Insert"]),
    ),
    Delete => command(
        "delete",
        "Delete",
        MenuPlace::at(Edit, 1),
        Keys::widget(&["Delete"]),
    ),
    SelectAll => command(
        "select-all",
        "Select All",
        MenuPlace::at(Edit, 2),
        Keys::widget(&["<Control>a"]),
    ),
    Find => command(
        "find",
        "Find…",
        MenuPlace::at(Search, 0),
        Keys::app(&["<Control>f"]),
    ),
    FindInFiles => command(
        "find-in-files",
        "Find in Files…",
        MenuPlace::at(Search, 0),
        Keys::app(&["<Control><Shift>f"]),
    ),
    // The F-key-free key comes first so that menus show it (ADR-009 amendment).
    FindNext => command(
        "find-next",
        "Find Next",
        MenuPlace::at(Search, 0),
        Keys::app(&["<Alt>Down", "F3"]),
    ),
    FindPrevious => command(
        "find-previous",
        "Find Previous",
        MenuPlace::at(Search, 0),
        Keys::app(&["<Alt>Up", "<Shift>F3"]),
    ),
    Replace => command(
        "replace",
        "Replace…",
        MenuPlace::at(Search, 0),
        Keys::app(&["<Control>h"]),
    ),
    // The find bar's buttons: no keys or menu entries, but the palette reaches them.
    ReplaceNext => command("replace-next", "Replace Next", None, Keys::NONE),
    ReplaceAll => command("replace-all", "Replace All", None, Keys::NONE),
    ReplaceAllInOpenDocuments => command(
        "replace-all-in-open-documents",
        "Replace All in Open Documents",
        None,
        Keys::NONE,
    ),
    Count => command("count", "Count", None, Keys::NONE),
    FindAllInDocument => command(
        "find-all-in-document",
        "Find All in Current Document",
        None,
        Keys::NONE,
    ),
    FindAllInOpenDocuments => command(
        "find-all-in-open-documents",
        "Find All in Open Documents",
        None,
        Keys::NONE,
    ),
    StopFindInFiles => command("stop-find-in-files", "Stop Find in Files", None, Keys::NONE),
    // The Find in Files row's other button.
    ReplaceInFiles => command("replace-in-files", "Replace in Files", None, Keys::NONE),
    // F7, F4 and Shift+F4 after an F-key-free key (ADR-009 amendment on search keys).
    SearchResults => command(
        "search-results",
        "Search Results Window",
        MenuPlace::at(Search, 1),
        Keys::app(&["<Control><Alt>r", "F7"]),
    ),
    NextSearchResult => command(
        "next-search-result",
        "Next Search Result",
        MenuPlace::at(Search, 1),
        Keys::app(&["<Control><Alt>Down", "F4"]),
    ),
    PreviousSearchResult => command(
        "previous-search-result",
        "Previous Search Result",
        MenuPlace::at(Search, 1),
        Keys::app(&["<Control><Alt>Up", "<Shift>F4"]),
    ),
    CopySearchResults => command(
        "copy-search-results",
        "Copy Search Results",
        None,
        Keys::NONE,
    ),
    ClearSearchResults => command(
        "clear-search-results",
        "Clear Search Results",
        None,
        Keys::NONE,
    ),
    CloseSearchResults => command(
        "close-search-results",
        "Close Search Results",
        None,
        Keys::NONE,
    ),
    GoToLine => command(
        "go-to-line",
        "Go to Line…",
        MenuPlace::at(Search, 2),
        Keys::app(&["<Control>g"]),
    ),
    // Back and forward through the places jumps left: the browsers' Alt+Left and
    // Alt+Right, which GtkSourceView would use to move words (ADR-009 amendment).
    GoBack => command(
        "go-back",
        "Go Back",
        MenuPlace::at(Search, 2),
        Keys::app(&["<Alt>Left"]),
    ),
    GoForward => command(
        "go-forward",
        "Go Forward",
        MenuPlace::at(Search, 2),
        Keys::app(&["<Alt>Right"]),
    ),
    WordWrap => toggle("word-wrap", "Word Wrap", MenuPlace::at(View, 0)),
    ShowWhitespace => toggle("show-whitespace", "Show Whitespace", MenuPlace::at(View, 0)),
    // `+` is unshifted on the Norwegian layout and `=` on the US one.
    ZoomIn => command(
        "zoom-in",
        "Zoom In",
        MenuPlace::at(View, 1),
        Keys::app(&["<Control>plus", "<Control>equal", "<Control>KP_Add"]),
    ),
    ZoomOut => command(
        "zoom-out",
        "Zoom Out",
        MenuPlace::at(View, 1),
        Keys::app(&["<Control>minus", "<Control>KP_Subtract"]),
    ),
    ZoomReset => command(
        "zoom-reset",
        "Reset Zoom",
        MenuPlace::at(View, 1),
        Keys::app(&["<Control>0", "<Control>KP_Divide"]),
    ),
    FullScreen => ActionSpec {
        keys: Keys::app(&["<Alt>Return", "F11"]),
        ..toggle("full-screen", "Full Screen", MenuPlace::at(View, 2))
    },
    // Tab order on Ctrl+PgDn and Ctrl+PgUp; Ctrl+Tab is the MRU switcher.
    NextTab => command(
        "next-tab",
        "Next Tab",
        MenuPlace::at(View, 3),
        Keys::app(&["<Control>Page_Down"]),
    ),
    PreviousTab => command(
        "previous-tab",
        "Previous Tab",
        MenuPlace::at(View, 3),
        Keys::app(&["<Control>Page_Up"]),
    ),
    // Most recently used order while Ctrl is held; a tap goes to the previous tab
    // (ADR-009 amendment on navigation keys).
    NextRecentTab => command(
        "next-recent-tab",
        "Next Recent Tab",
        MenuPlace::at(View, 3),
        Keys::app(&["<Control>Tab"]),
    ),
    PreviousRecentTab => command(
        "previous-recent-tab",
        "Previous Recent Tab",
        MenuPlace::at(View, 3),
        Keys::app(&["<Control><Shift>Tab"]),
    ),
    MoveTabForward => command(
        "move-tab-forward",
        "Move Tab Forward",
        MenuPlace::at(View, 3),
        Keys::app(&["<Control><Shift>Page_Down"]),
    ),
    MoveTabBackward => command(
        "move-tab-backward",
        "Move Tab Backward",
        MenuPlace::at(View, 3),
        Keys::app(&["<Control><Shift>Page_Up"]),
    ),
    // The tab menu's pin entries; only the one that applies is enabled.
    PinTab => command("pin-tab", "Pin Tab", MenuPlace::at(View, 3), Keys::NONE),
    UnpinTab => command("unpin-tab", "Unpin Tab", MenuPlace::at(View, 3), Keys::NONE),
    CommandPalette => command(
        "command-palette",
        "Command Palette…",
        MenuPlace::at(View, 5),
        Keys::app(&["<Control><Shift>p", "F1"]),
    ),
    ChooseLanguage => command(
        "choose-language",
        "Set Language…",
        MenuPlace::at(Language, 0),
        Keys::NONE,
    ),
    SetLanguage => ActionSpec {
        kind: ActionKind::WithString,
        palette: false,
        ..command("set-language", "Set Language", None, Keys::NONE)
    },
    ReloadFromDisk => command(
        "reload-from-disk",
        "Reload from Disk",
        MenuPlace::at(File, 0),
        Keys::NONE,
    ),
    // Edit › EOL Conversion; the buffer keeps LF (ADR-008).
    EolCrLf => command(
        "eol-crlf",
        "Windows (CR LF)",
        MenuPlace::nested_in(Edit, 3, EOL_SUBMENU, 0),
        Keys::NONE,
    ),
    EolLf => command(
        "eol-lf",
        "Unix (LF)",
        MenuPlace::nested_in(Edit, 3, EOL_SUBMENU, 0),
        Keys::NONE,
    ),
    EolCr => command(
        "eol-cr",
        "Macintosh (CR)",
        MenuPlace::nested_in(Edit, 3, EOL_SUBMENU, 0),
        Keys::NONE,
    ),
    // The Encoding menu: reinterpret the file's bytes ("Encode in"), any character
    // set, then convert what the next save writes (ADR-007).
    EncodeUtf8 => command(
        "encode-utf8",
        "Encode in UTF-8",
        MenuPlace::at(Encoding, 0),
        Keys::NONE,
    ),
    EncodeUtf8Bom => command(
        "encode-utf8-bom",
        "Encode in UTF-8-BOM",
        MenuPlace::at(Encoding, 0),
        Keys::NONE,
    ),
    EncodeUtf16BeBom => command(
        "encode-utf16be-bom",
        "Encode in UTF-16 BE BOM",
        MenuPlace::at(Encoding, 0),
        Keys::NONE,
    ),
    EncodeUtf16LeBom => command(
        "encode-utf16le-bom",
        "Encode in UTF-16 LE BOM",
        MenuPlace::at(Encoding, 0),
        Keys::NONE,
    ),
    Reinterpret => with_encoding(
        "reinterpret",
        "Reinterpret As",
        MenuPlace::nested(Encoding, CHARACTER_SETS_SUBMENU, 0),
    ),
    ConvertToAnsi => command(
        "convert-to-ansi",
        "Convert to ANSI (Windows-1252)",
        MenuPlace::at(Encoding, 1),
        Keys::NONE,
    ),
    ConvertToUtf8 => command(
        "convert-to-utf8",
        "Convert to UTF-8",
        MenuPlace::at(Encoding, 1),
        Keys::NONE,
    ),
    ConvertToUtf8Bom => command(
        "convert-to-utf8-bom",
        "Convert to UTF-8-BOM",
        MenuPlace::at(Encoding, 1),
        Keys::NONE,
    ),
    ConvertToUtf16BeBom => command(
        "convert-to-utf16be-bom",
        "Convert to UTF-16 BE BOM",
        MenuPlace::at(Encoding, 1),
        Keys::NONE,
    ),
    ConvertToUtf16LeBom => command(
        "convert-to-utf16le-bom",
        "Convert to UTF-16 LE BOM",
        MenuPlace::at(Encoding, 1),
        Keys::NONE,
    ),
    ConvertEncoding => with_encoding("convert-encoding", "Convert To", None),
    ChooseEncoding => command("choose-encoding", "Encoding…", None, Keys::NONE),
    // The tab strip's context menu and its File-menu places.
    RenameFile => command("rename-file", "Rename…", MenuPlace::at(File, 1), Keys::NONE),
    MoveToTrash => command(
        "move-to-trash",
        "Move to Trash…",
        MenuPlace::at(File, 3),
        Keys::NONE,
    ),
    OpenContainingFolder => command(
        "open-containing-folder",
        "Open Containing Folder",
        MenuPlace::at(File, 2),
        Keys::NONE,
    ),
    OpenTerminalHere => command(
        "open-terminal-here",
        "Open Terminal Here",
        MenuPlace::at(File, 2),
        Keys::NONE,
    ),
    CloseOthers => command(
        "close-others",
        "Close Others",
        MenuPlace::at(File, 3),
        Keys::NONE,
    ),
    CloseToTheRight => command(
        "close-to-the-right",
        "Close to the Right",
        MenuPlace::at(File, 3),
        Keys::NONE,
    ),
    CopyFullPath => command(
        "copy-full-path",
        "Copy Full Path",
        MenuPlace::nested_in(Edit, 3, COPY_SUBMENU, 0),
        Keys::NONE,
    ),
    CopyFileName => command(
        "copy-file-name",
        "Copy File Name",
        MenuPlace::nested_in(Edit, 3, COPY_SUBMENU, 0),
        Keys::NONE,
    ),
    CopyDirectoryPath => command(
        "copy-directory-path",
        "Copy Directory Path",
        MenuPlace::nested_in(Edit, 3, COPY_SUBMENU, 0),
        Keys::NONE,
    ),
    // fcitx5 takes Ctrl+Shift+U and Ctrl+Alt+Shift+U, so UPPERCASE is Alt+Shift+U and
    // the blend variants have no key (ADR-009 amendment on text-tool keys).
    Uppercase => editing(
        "uppercase",
        "UPPERCASE",
        MenuPlace::nested_in(Edit, 3, CASE_SUBMENU, 0),
        Keys::app(&["<Alt><Shift>u"]),
    ),
    Lowercase => editing(
        "lowercase",
        "lowercase",
        MenuPlace::nested_in(Edit, 3, CASE_SUBMENU, 0),
        Keys::app(&["<Control>u"]),
    ),
    ProperCase => editing(
        "proper-case",
        "Proper Case",
        MenuPlace::nested_in(Edit, 3, CASE_SUBMENU, 0),
        Keys::app(&["<Alt>u"]),
    ),
    ProperCaseBlend => editing(
        "proper-case-blend",
        "Proper Case (blend)",
        MenuPlace::nested_in(Edit, 3, CASE_SUBMENU, 0),
        Keys::NONE,
    ),
    SentenceCase => editing(
        "sentence-case",
        "Sentence case",
        MenuPlace::nested_in(Edit, 3, CASE_SUBMENU, 0),
        Keys::app(&["<Control><Alt>u"]),
    ),
    SentenceCaseBlend => editing(
        "sentence-case-blend",
        "Sentence case (blend)",
        MenuPlace::nested_in(Edit, 3, CASE_SUBMENU, 0),
        Keys::NONE,
    ),
    InvertCase => editing(
        "invert-case",
        "iNVERT cASE",
        MenuPlace::nested_in(Edit, 3, CASE_SUBMENU, 0),
        Keys::NONE,
    ),
    DuplicateLine => editing(
        "duplicate-line",
        "Duplicate Current Line",
        MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 0),
        Keys::app(&["<Control>d"]),
    ),
    CutLine => editing(
        "cut-line",
        "Cut Current Line",
        MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 0),
        Keys::app(&["<Control>l"]),
    ),
    CopyLine => editing(
        "copy-line",
        "Copy Current Line",
        MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 0),
        Keys::app(&["<Control><Shift>x"]),
    ),
    DeleteLine => editing(
        "delete-line",
        "Delete Current Line",
        MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 0),
        Keys::app(&["<Control><Shift>l"]),
    ),
    TransposeLine => editing(
        "transpose-line",
        "Transpose Current Line",
        MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 0),
        Keys::app(&["<Control>t"]),
    ),
    MoveLineUp => editing(
        "move-line-up",
        "Move Up Current Line",
        MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 1),
        Keys::app(&["<Control><Shift>Up"]),
    ),
    MoveLineDown => editing(
        "move-line-down",
        "Move Down Current Line",
        MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 1),
        Keys::app(&["<Control><Shift>Down"]),
    ),
    JoinLines => editing(
        "join-lines",
        "Join Lines",
        MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 1),
        Keys::app(&["<Control>j"]),
    ),
    BlankLineAbove => editing(
        "blank-line-above",
        "Insert Blank Line Above Current",
        MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 1),
        Keys::app(&["<Control><Alt>Return"]),
    ),
    BlankLineBelow => editing(
        "blank-line-below",
        "Insert Blank Line Below Current",
        MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 1),
        Keys::app(&["<Control><Alt><Shift>Return"]),
    ),
    RemoveDuplicateLines => editing(
        "remove-duplicate-lines",
        "Remove Duplicate Lines",
        MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 2),
        Keys::NONE,
    ),
    RemoveConsecutiveDuplicateLines => editing(
        "remove-consecutive-duplicate-lines",
        "Remove Consecutive Duplicate Lines",
        MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 2),
        Keys::NONE,
    ),
    RemoveEmptyLines => editing(
        "remove-empty-lines",
        "Remove Empty Lines",
        MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 2),
        Keys::NONE,
    ),
    RemoveBlankLines => editing(
        "remove-blank-lines",
        "Remove Empty Lines (Containing Blank Characters)",
        MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 2),
        Keys::NONE,
    ),
    ReverseLines => editing(
        "reverse-lines",
        "Reverse Line Order",
        MenuPlace::nested_in(Edit, 3, LINES_SUBMENU, 2),
        Keys::NONE,
    ),
    SortLexicalAscending => sort(
        "sort-lexical-ascending",
        "Sort Lines Lexicographically Ascending",
    ),
    SortLexicalDescending => sort(
        "sort-lexical-descending",
        "Sort Lines Lexicographically Descending",
    ),
    SortIgnoreCaseAscending => sort(
        "sort-ignore-case-ascending",
        "Sort Lines Lex. Ascending Ignoring Case",
    ),
    SortIgnoreCaseDescending => sort(
        "sort-ignore-case-descending",
        "Sort Lines Lex. Descending Ignoring Case",
    ),
    SortIntegerAscending => sort("sort-integer-ascending", "Sort Lines As Integers Ascending"),
    SortIntegerDescending => sort(
        "sort-integer-descending",
        "Sort Lines As Integers Descending",
    ),
    SortDecimalCommaAscending => sort(
        "sort-decimal-comma-ascending",
        "Sort Lines As Decimals (Comma) Ascending",
    ),
    SortDecimalCommaDescending => sort(
        "sort-decimal-comma-descending",
        "Sort Lines As Decimals (Comma) Descending",
    ),
    SortDecimalDotAscending => sort(
        "sort-decimal-dot-ascending",
        "Sort Lines As Decimals (Dot) Ascending",
    ),
    SortDecimalDotDescending => sort(
        "sort-decimal-dot-descending",
        "Sort Lines As Decimals (Dot) Descending",
    ),
    SortLengthAscending => sort("sort-length-ascending", "Sort Lines By Length Ascending"),
    SortLengthDescending => sort("sort-length-descending", "Sort Lines By Length Descending"),
    ToggleComment => editing(
        "toggle-comment",
        "Toggle Single Line Comment",
        MenuPlace::nested_in(Edit, 3, COMMENT_SUBMENU, 0),
        Keys::app(&["<Control>q"]),
    ),
    CommentLines => editing(
        "comment-lines",
        "Single Line Comment",
        MenuPlace::nested_in(Edit, 3, COMMENT_SUBMENU, 0),
        Keys::app(&["<Control>k"]),
    ),
    UncommentLines => editing(
        "uncomment-lines",
        "Single Line Uncomment",
        MenuPlace::nested_in(Edit, 3, COMMENT_SUBMENU, 0),
        Keys::app(&["<Control><Shift>k"]),
    ),
    ToggleBlockComment => editing(
        "toggle-block-comment",
        "Block Comment",
        MenuPlace::nested_in(Edit, 3, COMMENT_SUBMENU, 1),
        Keys::app(&["<Control><Shift>q"]),
    ),
    TrimTrailing => editing(
        "trim-trailing",
        "Trim Trailing Space",
        MenuPlace::nested_in(Edit, 3, BLANK_SUBMENU, 0),
        Keys::NONE,
    ),
    TrimLeading => editing(
        "trim-leading",
        "Trim Leading Space",
        MenuPlace::nested_in(Edit, 3, BLANK_SUBMENU, 0),
        Keys::NONE,
    ),
    TrimBoth => editing(
        "trim-both",
        "Trim Leading and Trailing Space",
        MenuPlace::nested_in(Edit, 3, BLANK_SUBMENU, 0),
        Keys::NONE,
    ),
    TabsToSpaces => editing(
        "tabs-to-spaces",
        "TAB to Space",
        MenuPlace::nested_in(Edit, 3, BLANK_SUBMENU, 1),
        Keys::NONE,
    ),
    SpacesToTabs => editing(
        "spaces-to-tabs",
        "Space to TAB (All)",
        MenuPlace::nested_in(Edit, 3, BLANK_SUBMENU, 1),
        Keys::NONE,
    ),
    SpacesToTabsLeading => editing(
        "spaces-to-tabs-leading",
        "Space to TAB (Leading)",
        MenuPlace::nested_in(Edit, 3, BLANK_SUBMENU, 1),
        Keys::NONE,
    ),
    // Simpler forms of the Column Editor.
    AddPrefixSuffix => editing(
        "add-prefix-suffix",
        "Add Prefix/Suffix…",
        MenuPlace::at(Edit, 4),
        Keys::NONE,
    ),
    InsertNumbers => editing(
        "insert-numbers",
        "Insert Numbers…",
        MenuPlace::at(Edit, 4),
        Keys::NONE,
    ),
    // GtkSourceView's move-to-matching-bracket.
    GoToMatchingBrace => editing(
        "go-to-matching-brace",
        "Go to Matching Brace",
        MenuPlace::at(Search, 2),
        Keys::app(&["<Control>b"]),
    ),
    SelectToMatchingBrace => editing(
        "select-to-matching-brace",
        "Select to Matching Brace",
        MenuPlace::at(Search, 2),
        Keys::app(&["<Control><Alt>b"]),
    ),
    // Ctrl+F3 and Ctrl+Shift+F3 after an F-key-free key.
    SelectAndFindNext => editing(
        "select-and-find-next",
        "Select and Find Next",
        MenuPlace::at(Search, 0),
        Keys::app(&["<Control><Alt>f", "<Control>F3"]),
    ),
    SelectAndFindPrevious => editing(
        "select-and-find-previous",
        "Select and Find Previous",
        MenuPlace::at(Search, 0),
        Keys::app(&["<Control><Alt><Shift>f", "<Control><Shift>F3"]),
    ),
    DocumentMap => toggle("document-map", "Document Map", MenuPlace::at(View, 0)),
    // The status bar's indentation item opens the same popover.
    ChooseIndentation => command("choose-indentation", "Indentation…", None, Keys::NONE),
    DetectIndentation => command(
        "detect-indentation",
        "Detect Indentation from Content",
        None,
        Keys::NONE,
    ),
    OpenSettings => command(
        "open-settings",
        "Open Settings (config.toml)",
        MenuPlace::at(Settings, 0),
        Keys::NONE,
    ),
    OpenKeyboardShortcuts => command(
        "open-keyboard-shortcuts",
        "Open Keyboard Shortcuts (keys.toml)",
        MenuPlace::at(Settings, 0),
        Keys::NONE,
    ),
    SetAsDefaultEditor => command(
        "set-as-default-editor",
        "Set as Default Editor…",
        MenuPlace::at(Settings, 1),
        Keys::NONE,
    ),
    // The JSON and XML tools are on Ctrl+Alt+Shift, with J and X for the validations.
    FormatJson => editing(
        "format-json",
        "Format JSON",
        MenuPlace::nested(Tools, JSON_SUBMENU, 0),
        Keys::app(&["<Control><Alt><Shift>m"]),
    ),
    MinifyJson => editing(
        "minify-json",
        "Minify JSON",
        MenuPlace::nested(Tools, JSON_SUBMENU, 0),
        Keys::app(&["<Control><Alt><Shift>c"]),
    ),
    ValidateJson => editing(
        "validate-json",
        "Validate JSON",
        MenuPlace::nested(Tools, JSON_SUBMENU, 0),
        Keys::app(&["<Control><Alt><Shift>j"]),
    ),
    FormatXml => editing(
        "format-xml",
        "Format XML",
        MenuPlace::nested(Tools, XML_SUBMENU, 0),
        Keys::app(&["<Control><Alt><Shift>b"]),
    ),
    ValidateXml => editing(
        "validate-xml",
        "Validate XML",
        MenuPlace::nested(Tools, XML_SUBMENU, 0),
        Keys::app(&["<Control><Alt><Shift>x"]),
    ),
    KeyboardShortcuts => command(
        "keyboard-shortcuts",
        "Keyboard Shortcuts",
        MenuPlace::at(Help, 0),
        Keys::NONE,
    ),
    About => command("about", "About Stet", MenuPlace::at(Help, 0), Keys::NONE),
    CheckForUpdates => command(
        "check-for-updates",
        "Check for Updates…",
        MenuPlace::at(Main, 0),
        Keys::NONE,
    ),
    // Column selection on Alt+Shift takes over GtkSourceView's `move-viewport` bindings:
    // application accelerators run before the editor's own bindings (ADR-009, ADR-016).
    ColumnSelectLeft => command(
        "column-select-left",
        "Column Select Left",
        None,
        Keys::app(&["<Alt><Shift>Left", "<Alt><Shift>KP_Left"]),
    ),
    ColumnSelectRight => command(
        "column-select-right",
        "Column Select Right",
        None,
        Keys::app(&["<Alt><Shift>Right", "<Alt><Shift>KP_Right"]),
    ),
    ColumnSelectUp => command(
        "column-select-up",
        "Column Select Up",
        None,
        Keys::app(&["<Alt><Shift>Up", "<Alt><Shift>KP_Up"]),
    ),
    ColumnSelectDown => command(
        "column-select-down",
        "Column Select Down",
        None,
        Keys::app(&["<Alt><Shift>Down", "<Alt><Shift>KP_Down"]),
    ),
    ColumnSelectLineStart => command(
        "column-select-line-start",
        "Column Select to Line Start",
        None,
        Keys::app(&["<Alt><Shift>Home", "<Alt><Shift>KP_Home"]),
    ),
    ColumnSelectLineEnd => command(
        "column-select-line-end",
        "Column Select to Line End",
        None,
        Keys::app(&["<Alt><Shift>End", "<Alt><Shift>KP_End"]),
    ),
    ColumnSelectPageUp => command(
        "column-select-page-up",
        "Column Select Page Up",
        None,
        Keys::app(&["<Alt><Shift>Page_Up", "<Alt><Shift>KP_Page_Up"]),
    ),
    ColumnSelectPageDown => command(
        "column-select-page-down",
        "Column Select Page Down",
        None,
        Keys::app(&["<Alt><Shift>Page_Down", "<Alt><Shift>KP_Page_Down"]),
    ),
    ColumnBeginEndSelect => command(
        "column-begin-end-select",
        "Begin/End Select in Column Mode",
        MenuPlace::at(Edit, 2),
        Keys::app(&["<Alt><Shift>b"]),
    ),
    ColumnEditor => command(
        "column-editor",
        "Column Editor…",
        MenuPlace::at(Edit, 2),
        Keys::app(&["<Alt>c"]),
    ),
    // Search › Bookmark. Ctrl+F2, F2 and Shift+F2 come after the F-key-free keys of
    // VS Code's Bookmarks extension (ADR-009 amendment on marking keys).
    ToggleBookmark => in_search(
        "toggle-bookmark",
        "Toggle Bookmark",
        BOOKMARK_SUBMENU,
        0,
        Keys::app(&["<Control><Alt>k", "<Control>F2"]),
    ),
    NextBookmark => in_search(
        "next-bookmark",
        "Next Bookmark",
        BOOKMARK_SUBMENU,
        0,
        Keys::app(&["<Control><Alt>l", "F2"]),
    ),
    PreviousBookmark => in_search(
        "previous-bookmark",
        "Previous Bookmark",
        BOOKMARK_SUBMENU,
        0,
        Keys::app(&["<Control><Alt>j", "<Shift>F2"]),
    ),
    ClearBookmarks => in_search(
        "clear-bookmarks",
        "Clear All Bookmarks",
        BOOKMARK_SUBMENU,
        0,
        Keys::NONE,
    ),
    CutBookmarkedLines => in_search(
        "cut-bookmarked-lines",
        "Cut Bookmarked Lines",
        BOOKMARK_SUBMENU,
        1,
        Keys::NONE,
    ),
    CopyBookmarkedLines => in_search(
        "copy-bookmarked-lines",
        "Copy Bookmarked Lines",
        BOOKMARK_SUBMENU,
        1,
        Keys::NONE,
    ),
    PasteToBookmarkedLines => in_search(
        "paste-to-bookmarked-lines",
        "Paste to (Replace) Bookmarked Lines",
        BOOKMARK_SUBMENU,
        1,
        Keys::NONE,
    ),
    RemoveBookmarkedLines => in_search(
        "remove-bookmarked-lines",
        "Remove Bookmarked Lines",
        BOOKMARK_SUBMENU,
        1,
        Keys::NONE,
    ),
    RemoveUnbookmarkedLines => in_search(
        "remove-unbookmarked-lines",
        "Remove Non-Bookmarked Lines",
        BOOKMARK_SUBMENU,
        1,
        Keys::NONE,
    ),
    InverseBookmarks => in_search(
        "inverse-bookmarks",
        "Inverse Bookmarks",
        BOOKMARK_SUBMENU,
        1,
        Keys::NONE,
    ),
    // The find bar's Mark row.
    Mark => command(
        "mark",
        "Mark…",
        MenuPlace::at(Search, 3),
        Keys::app(&["<Control>m"]),
    ),
    MarkAll => command("mark-all", "Mark All", None, Keys::NONE),
    ClearMarks => command(
        "clear-marks",
        "Clear All Marks",
        MenuPlace::at(Search, 3),
        Keys::NONE,
    ),
    CopyMarkedText => command(
        "copy-marked-text",
        "Copy Marked Text",
        MenuPlace::at(Search, 3),
        Keys::NONE,
    ),
    StyleToken1 => in_search(
        "style-token-1",
        "Using 1st Style",
        STYLE_TOKEN_SUBMENU,
        0,
        Keys::NONE,
    ),
    StyleToken2 => in_search(
        "style-token-2",
        "Using 2nd Style",
        STYLE_TOKEN_SUBMENU,
        0,
        Keys::NONE,
    ),
    StyleToken3 => in_search(
        "style-token-3",
        "Using 3rd Style",
        STYLE_TOKEN_SUBMENU,
        0,
        Keys::NONE,
    ),
    StyleToken4 => in_search(
        "style-token-4",
        "Using 4th Style",
        STYLE_TOKEN_SUBMENU,
        0,
        Keys::NONE,
    ),
    StyleToken5 => in_search(
        "style-token-5",
        "Using 5th Style",
        STYLE_TOKEN_SUBMENU,
        0,
        Keys::NONE,
    ),
    ClearStyle1 => in_search(
        "clear-style-1",
        "Clear 1st Style",
        CLEAR_STYLE_SUBMENU,
        0,
        Keys::NONE,
    ),
    ClearStyle2 => in_search(
        "clear-style-2",
        "Clear 2nd Style",
        CLEAR_STYLE_SUBMENU,
        0,
        Keys::NONE,
    ),
    ClearStyle3 => in_search(
        "clear-style-3",
        "Clear 3rd Style",
        CLEAR_STYLE_SUBMENU,
        0,
        Keys::NONE,
    ),
    ClearStyle4 => in_search(
        "clear-style-4",
        "Clear 4th Style",
        CLEAR_STYLE_SUBMENU,
        0,
        Keys::NONE,
    ),
    ClearStyle5 => in_search(
        "clear-style-5",
        "Clear 5th Style",
        CLEAR_STYLE_SUBMENU,
        0,
        Keys::NONE,
    ),
    ClearAllStyles => in_search(
        "clear-all-styles",
        "Clear All Styles",
        CLEAR_STYLE_SUBMENU,
        1,
        Keys::NONE,
    ),
    // Ctrl+1–5 jump down and Ctrl+Alt+1–5 up: Ctrl+Shift+1–5 can't be matched on every
    // layout (Shift turns the digit into a symbol). Ctrl+0 is Reset Zoom, so the Find
    // Mark Style jumps are Alt+M and Alt+Shift+M (ADR-009 amendment on marking keys).
    JumpUp1 => jump(
        "jump-up-1",
        "1st Style",
        JUMP_UP_SUBMENU,
        0,
        Keys::app(&["<Control><Alt>1"]),
    ),
    JumpUp2 => jump(
        "jump-up-2",
        "2nd Style",
        JUMP_UP_SUBMENU,
        0,
        Keys::app(&["<Control><Alt>2"]),
    ),
    JumpUp3 => jump(
        "jump-up-3",
        "3rd Style",
        JUMP_UP_SUBMENU,
        0,
        Keys::app(&["<Control><Alt>3"]),
    ),
    JumpUp4 => jump(
        "jump-up-4",
        "4th Style",
        JUMP_UP_SUBMENU,
        0,
        Keys::app(&["<Control><Alt>4"]),
    ),
    JumpUp5 => jump(
        "jump-up-5",
        "5th Style",
        JUMP_UP_SUBMENU,
        0,
        Keys::app(&["<Control><Alt>5"]),
    ),
    JumpUpMark => jump(
        "jump-up-mark",
        "Find Mark Style",
        JUMP_UP_SUBMENU,
        1,
        Keys::app(&["<Alt><Shift>m"]),
    ),
    JumpDown1 => jump(
        "jump-down-1",
        "1st Style",
        JUMP_DOWN_SUBMENU,
        0,
        Keys::app(&["<Control>1"]),
    ),
    JumpDown2 => jump(
        "jump-down-2",
        "2nd Style",
        JUMP_DOWN_SUBMENU,
        0,
        Keys::app(&["<Control>2"]),
    ),
    JumpDown3 => jump(
        "jump-down-3",
        "3rd Style",
        JUMP_DOWN_SUBMENU,
        0,
        Keys::app(&["<Control>3"]),
    ),
    JumpDown4 => jump(
        "jump-down-4",
        "4th Style",
        JUMP_DOWN_SUBMENU,
        0,
        Keys::app(&["<Control>4"]),
    ),
    JumpDown5 => jump(
        "jump-down-5",
        "5th Style",
        JUMP_DOWN_SUBMENU,
        0,
        Keys::app(&["<Control>5"]),
    ),
    JumpDownMark => jump(
        "jump-down-mark",
        "Find Mark Style",
        JUMP_DOWN_SUBMENU,
        1,
        Keys::app(&["<Alt>m"]),
    ),
    // View › Move/Clone Current Document, Focus on Another View (F8, after an F-key-free
    // key) and the synchronised scrolling toggles.
    MoveToOtherView => command(
        "move-to-other-view",
        "Move to Other View",
        MenuPlace::nested_in(View, 4, MOVE_CLONE_SUBMENU, 0),
        Keys::NONE,
    ),
    CloneToOtherView => command(
        "clone-to-other-view",
        "Clone to Other View",
        MenuPlace::nested_in(View, 4, MOVE_CLONE_SUBMENU, 0),
        Keys::NONE,
    ),
    SwitchView => command(
        "switch-view",
        "Focus on Another View",
        MenuPlace::at(View, 4),
        Keys::app(&["<Control><Alt>o", "F8"]),
    ),
    SyncVerticalScrolling => toggle(
        "sync-vertical-scrolling",
        "Synchronise Vertical Scrolling",
        MenuPlace::at(View, 4),
    ),
    SyncHorizontalScrolling => toggle(
        "sync-horizontal-scrolling",
        "Synchronise Horizontal Scrolling",
        MenuPlace::at(View, 4),
    ),
    Compare => in_compare("compare", "Compare", 0, Keys::app(&["<Control><Alt>c"])),
    CompareWithFile => in_compare("compare-with-file", "Compare with File…", 0, Keys::NONE),
    CompareWithClipboard => in_compare(
        "compare-with-clipboard",
        "Compare with Clipboard",
        0,
        Keys::app(&["<Control><Alt>m"]),
    ),
    CompareWithSaved => in_compare(
        "compare-with-saved",
        "Compare with Saved Version",
        0,
        Keys::app(&["<Control><Alt>d"]),
    ),
    PreviousDifference => in_compare(
        "previous-difference",
        "Previous Difference",
        1,
        Keys::app(&["<Alt>Page_Up"]),
    ),
    NextDifference => in_compare(
        "next-difference",
        "Next Difference",
        1,
        Keys::app(&["<Alt>Page_Down"]),
    ),
    CompareIgnoreWhitespace => toggle(
        "compare-ignore-whitespace",
        "Ignore Whitespace",
        MenuPlace::nested(Tools, COMPARE_SUBMENU, 2),
    ),
    CompareIgnoreCase => toggle(
        "compare-ignore-case",
        "Ignore Case",
        MenuPlace::nested(Tools, COMPARE_SUBMENU, 2),
    ),
    ClearCompare => in_compare(
        "clear-compare",
        "Clear Compare",
        3,
        Keys::app(&["<Control><Alt>x"]),
    ),
}

#[cfg(test)]
mod tests {
    use crate::actions::menu::EOL_SUBMENU;
    use crate::actions::{
        Accelerator, ActionId, ActionKind, ActionScope, KeyScope, Menu, MenuPlace,
    };
    use std::collections::{HashMap, HashSet};

    /// How a punctuation key is typed on a layout.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Level {
        Base,
        Shift,
        AltGr,
    }

    /// The punctuation keys the keymap audit covers, on the US and the Norwegian (`no`) layout
    /// (from xkeyboard-config's `us` and `no(basic)`). Letters, digits, the keypad, arrows and
    /// F-keys are the same on both.
    const PUNCTUATION_LEVELS: &[(&str, Level, Level)] = &[
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
    fn punctuation_levels(key: &str) -> Option<(Level, Level)> {
        PUNCTUATION_LEVELS
            .iter()
            .find(|(name, _, _)| *name == key)
            .map(|&(_, us, no)| (us, no))
    }

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
        // The text tools.
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
        // fcitx5 takes Ctrl+Shift+U (ADR-009 amendment on text-tool keys).
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
        assert_eq!(
            ActionId::CheckForUpdates.spec().menu,
            MenuPlace::at(Menu::Main, 0)
        );
        assert_eq!(
            ActionId::CheckForUpdates.menu_path().as_deref(),
            Some("Menu")
        );
        assert!(!Menu::ALL.contains(&Menu::Main));
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
}
