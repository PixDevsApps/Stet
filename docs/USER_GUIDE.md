# Stet user guide

Project: Stet, a fast, keyboard-first text and code editor for Omarchy

Synced: 2026-10-01 (1.3)

This guide describes Stet 1.0: the MVP (M1–M5, 0.3), column mode (M6), bookmarks, Mark, split view and compare (M7), and navigation and Replace in Files (M8). Installing Stet is in [INSTALL.md](INSTALL.md); how it fits into Omarchy is in [INTEGRATIONS.md](INTEGRATIONS.md).

---

## The window

```
+------------------------------------------------------------------------------+
| [=] | main.rs  x | ● notes.txt  x | Cargo.toml  x |                          |  menu, tab strip
+------------------------------------------------------------------------------+
| File changed on disk.                              [Reload] [Keep Mine]      |  banners (when needed)
+-----+-------------------------------------------------------------+----------+
|   1 | fn main() {                                                 | :::::::: |
|   2 |     println!("hello");                                      | ::::     |  editor (+ document map)
+-----+-------------------------------------------------------------+----------+
| [Find________]  Aa  W  Sel  [Normal v]  ^ v  3 of 12            ⋮  x         |  find bar (Ctrl+F, Ctrl+H)
+------------------------------------------------------------------------------+
| Search "TODO" (2 hits in 2 files)                     Stop  Copy  Clear  x   |  results panel
+------------------------------------------------------------------------------+
| Ln 2, Col 5, Sel 0 | 3 lines, 38 chars | Rust | LF | UTF-8 | Spaces: 4 | INS |  status bar
+------------------------------------------------------------------------------+
```

- **Tab strip.** One tab per document; `●` marks unsaved changes. Drag tabs to reorder them. Double-click a tab to rename it, as Rename… does: a file is renamed on disk; an untitled tab only gets the name, and nothing is saved. Right-click a tab for its menu: Close, Close Others, Close to the Right; Save, Save As…; Copy Full Path, Copy File Name, Copy Directory Path; Open Containing Folder (your file manager) and Open Terminal Here (your terminal, through `xdg-terminal-exec`); Rename… and Move to Trash…. These act on the tab you clicked.
- **The menu** (☰, the left end of the tab strip) has nine submenus: File, Edit, Search, View, Encoding, Language, Settings, Tools, Help. Every command is also in the **command palette** (Ctrl+Shift+P or F1), with its keys.
- **Banners** above the text say when something needs your attention: a file changed on disk, a read-only file, an encoding that could not be read back exactly, mixed line endings, a large file.
- **The editor's context menu** (right-click) has GtkTextView's clipboard items and, below them, Convert Case to, Comment/Uncomment, Line Operations, Select and Find Next and Go to Matching Brace.
- **The status bar** shows the caret's line and column and the selection (`Sel 12 | 2` is 12 characters on 2 lines), the document's lines and characters, the words if you ask for them (`word_count` in [config.toml](#configtoml)), and four buttons: the language (the language picker), the line ending (convert it), the encoding (reinterpret or convert), the indentation (below), then INS or OVR (the Insert key).
- **The document map** (View › Document Map) shows the whole document beside the text; it is off by default and never shown for files of 50 MiB or more.

The keyboard focus goes back to the text after every dialog, popover and menu.

## Files

- **Open** with Ctrl+O, File › Open Recent, the palette, a drag from the file manager, or `stet file.txt:40:5` in a terminal (line 40, column 5; a second `stet` opens its files in the running window).
- **Save** with Ctrl+S, Save As with Ctrl+Alt+S, Save All with Ctrl+Shift+S. A tab's right-click menu also has Save and Save As… for that tab. Saving keeps the file's encoding, byte-order mark, line endings, permissions and symbolic links, and never leaves a half-written file.
- **Encodings and line endings**: the status bar's encoding button reinterprets a file in another encoding or converts what the next save writes; the line-ending button converts between LF, CRLF and CR. A character the target encoding lacks is shown with its line and column before anything is written.
- **Large files** load in pieces while the window keeps working. From 50 MiB Stet switches to large-file mode (no highlighting, word wrap, document map or highlight-all); from 256 MiB it refuses (and earlier when memory is short). A file with a line over 50,000 characters asks first: Open Formatted (JSON and XML) or Open Read-Only.
- **Changes on disk** reload a clean document quietly (Undo brings the previous text back); with unsaved changes a banner asks.

## Unsaved work and sessions

You never have to save before quitting. Every 7 seconds Stet backs up the text of every document with unsaved changes and every untitled one, and closing the window (SUPER+W, Ctrl+Alt+Q), logging out or a crash brings everything back the next time Stet starts: the tabs in their order, the one in front, the unsaved text, the caret, the window's size, the zoom, the recent files and the find histories. Only closing a single tab with unsaved changes (Ctrl+W) asks first.

- Tabs other than the one in front load when you first show them.
- If a file changed on disk after its unsaved changes were kept, a banner offers **Keep Mine** or **Load Disk Version**.
- **File › Forget Unsaved Drafts…** (after a question) closes the untitled documents, puts files back as they are on disk and deletes the backups.
- `backups = false` in [config.toml](#configtoml) keeps unsaved text off the disk entirely: closing the window then asks about unsaved changes, and logging out loses them. `backup_interval` sets the seconds between backups.
- Backups live in `~/.local/state/stet/`, readable only by you. `stet --no-session` starts without a session.
- `stet --wait file` waits until that file's tab is closed, so `GIT_EDITOR="stet --wait"` and `SUDO_EDITOR="stet --wait"` work whether Stet is running or not; closing the tab with Discard makes it exit with status 1 (git then aborts the commit).

## Search

Ctrl+F opens the find bar, Ctrl+H adds the replace row, Ctrl+Shift+F the Find in Files row. Modes: Normal, Extended (`\n`, `\t`, `\xHH`, …) and Regex (PCRE2, with Boost-style `$1`, `\U…\E` replacement templates). Find Next and Previous are Alt+Down and Alt+Up (F3 and Shift+F3 with Fn), Enter and Shift+Enter in the field. Find All in the current or every open document and Find in Files fill the results panel (Ctrl+Alt+R or F7 to go there and back; Ctrl+Alt+Down and Up, or F4 and Shift+F4, step through the results).

**Select and Find Next** (Ctrl+Alt+F, Ctrl+F3) takes the selection, or the word at the caret, as a plain-text query and selects its next occurrence; **Select and Find Previous** is Ctrl+Alt+Shift+F (Ctrl+Shift+F3). Selecting a whole word highlights its other occurrences (smart highlighting).

**Replace in Files** is the Find in Files row's other button. It asks first, naming the folder and the filters, because files on disk can't be undone. Each file keeps its encoding, byte-order mark and line endings, and only the matches change; files it can't write as they were (binary, an encoding that can't hold the replacement, read-only, 50 MiB or more, changed meanwhile) are left alone and listed with the reason. A file open in a tab gets the change as one undoable edit instead: saved at once if it had no unsaved changes, left unsaved if it had.

## Navigation

- **Quick Open** (Ctrl+P) lists the open documents, the recent files, and the files of the project you are in (the folder up the tree that holds `.git`, `.gitignore` respected, hidden and binary files left out). Type a few letters of a name; `name:40` opens it at line 40, and `:40` goes to line 40 of the current document.
- **Ctrl+Tab** switches to the document you used before; hold Ctrl and press Tab again to go further back in the order you used them, and let go of Ctrl to switch (Escape cancels). Ctrl+PgDn and Ctrl+PgUp go through the tabs in strip order.
- **Go Back** and **Go Forward** (Alt+Left and Alt+Right, or the mouse's back and forward buttons) return to where you were before a jump: Go to Line, a search, a result, quick open, a tab switch, a brace jump.
- **Pin Tab** (the tab's menu, the View menu, the palette) keeps a tab at the start of the strip, shown as an icon; Close All, Close Others and Close to the Right leave pinned tabs open.
- A new document is named after its first line until you save it, and Save As proposes that name. To give it a name of your own without saving, double-click its tab (or use Rename… in the tab menu, the File menu or the palette): the tab keeps that name as the text changes, Save As proposes it, and the session keeps it. Rename… with an empty name goes back to the first line.

## Text tools

All of them are one undo step, also on large documents: up to 512 KiB they run at once, larger documents on a background thread, and a large result goes in a piece at a time while the window keeps painting. Their keys work while the editor has the keyboard focus.

**What they work on.** With a selection: the selection, or the lines it touches. Without one: Duplicate, Cut, Copy, Delete, Transpose, Move, Join, the blank lines, the case conversions (the word at the caret) and the comments work on the caret's line; sorts, the line removals, Reverse, trims, TAB and space conversions, Add Prefix/Suffix and the JSON and XML tools work on the whole document.

- **Edit › Line Operations**: Duplicate Current Line (Ctrl+D), Cut Current Line (Ctrl+L), Copy Current Line (Ctrl+Shift+X), Delete Current Line (Ctrl+Shift+L), Transpose Current Line (Ctrl+T: swaps it with the line above), Move Up and Move Down (Ctrl+Shift+Up and Down), Join Lines (Ctrl+J), Insert Blank Line Above and Below (Ctrl+Alt+Enter and Ctrl+Alt+Shift+Enter), Remove Duplicate Lines, Remove Consecutive Duplicate Lines, Remove Empty Lines (also with blank characters), Reverse Line Order, and the sorts: lexicographic (with or without case), as integers, as decimals with a dot or a comma, by length, ascending or descending. Every sort is stable.
- **Ctrl+C and Ctrl+X without a selection** copy and cut the caret's whole line.
- **Edit › Convert Case to**: UPPERCASE (Alt+Shift+U, see the [note on fcitx5](#uppercase-and-fcitx5)), lowercase (Ctrl+U), Proper Case (Alt+U), Proper Case (blend), Sentence case (Ctrl+Alt+U), Sentence case (blend), iNVERT cASE.
- **Edit › Comment/Uncomment**: Toggle Single Line Comment (Ctrl+Q), Single Line Comment (Ctrl+K), Single Line Uncomment (Ctrl+Shift+K), Block Comment (Ctrl+Shift+Q, a toggle). The tokens come from the document's language (status bar); plain text has none.
- **Edit › Blank Operations**: Trim Trailing, Leading, or Leading and Trailing Space; TAB to Space; Space to TAB (all, or leading only), with the document's tab width.
- **Edit › Add Prefix/Suffix…** adds text at the start and end of every line. **Edit › Insert Numbers…** inserts a number on every line at the caret's column, from the caret's line to the end of the document (or on the lines of the selection): start, step, how many lines share a number, base (decimal, hexadecimal, octal, binary), uppercase hex digits, and leading zeros or spaces up to a minimum width. Both dialogs remember their last values; Tab moves between the fields, Enter applies and Escape cancels.
- **Tools › JSON**: Format (Ctrl+Alt+Shift+M), Minify (Ctrl+Alt+Shift+C), Validate (Ctrl+Alt+Shift+J). **Tools › XML**: Format (Ctrl+Alt+Shift+B), Validate (Ctrl+Alt+Shift+X). Formatting indents with the document's indentation and copies every token as it is; an error moves the caret to it and says what is wrong.
- **Search › Go to Matching Brace** (Ctrl+B) and **Select to Matching Brace** (Ctrl+Alt+B) need the caret next to a bracket.

## Column mode

Column mode selects and edits a rectangle: the same columns on several lines.

- **Select** with Alt+Shift+arrows (also Home, End, PgUp and PgDn), or Alt+drag with the mouse; Alt+click starts at a point. Alt+Shift+B starts a rectangle at the caret and, pressed again, ends it there. The status bar shows the cursor corner and the size, `Sel 4×12` (rows × columns). Columns past the end of a line are allowed: typing there pads the line with spaces.
- **Edit** by typing, Backspace, Delete or Tab: the change goes in on every line of the rectangle, and a burst of typing is one undo step. Typing over a rectangle replaces it; Insert switches to overwrite. Input methods and compose keys work the same way.
- **Copy, cut and paste** a rectangle as a block; it pastes as a rectangle again, also at an ordinary caret. Plain text from another program: one line goes in on every row, several lines go in as a block.
- **Leave** with Escape, a plain arrow key or a click. Commands that don't know about columns (Find, Select All, Go to Line) leave the rectangle's corners selected as an ordinary selection.
- **Column Editor** (Alt+C, Edit › Column Editor…): inserts text, or numbers (initial, increase by, repeat, leading zeros or spaces, decimal, hexadecimal, octal or binary), on every line of the rectangle, or from the caret's line to the end of the document. One undo step.

A tab that the rectangle's edge cuts through is turned into spaces on the lines that change, so everything lines up as it looks.

## Bookmarks and Mark

**Bookmarks** mark lines: a dot in the gutter, left of the line numbers.

- **Toggle Bookmark** (Ctrl+Alt+K, or Ctrl+F2) on the caret's line, or click in the gutter. **Next Bookmark** (Ctrl+Alt+L, F2) and **Previous Bookmark** (Ctrl+Alt+J, Shift+F2) go round the document; Go Back (Alt+Left) returns from them. All of them are in **Search › Bookmark**, with Clear All Bookmarks and Inverse Bookmarks.
- **Search › Bookmark** also works on the bookmarked lines: Cut, Copy and Remove Bookmarked Lines, Remove Non-Bookmarked Lines, and Paste to (Replace) Bookmarked Lines, which replaces every bookmarked line with the clipboard's text. Each is one undo step, also on large documents, and Undo and Redo bring the bookmarks back with the lines.
- Bookmarks move with their lines as you type, and through Replace All, the text tools and a reload from disk. The session keeps them.

**Mark** (Search › Mark…, Ctrl+M) is a row of the find bar. **Mark All** (or Enter in the find field) marks every match of the query, with the find bar's mode (Normal, Extended, Regex) and options (Match case, Whole word, In selection), in the chosen style: the Find Mark Style or one of the five token styles.
- **Bookmark line** also bookmarks every line with a match; **Purge for each search** clears the style's marks (and, with Bookmark line, the bookmarks) first.
- **Clear All Marks** clears the style (and, with Bookmark line, the bookmarks); **Copy Marked Text** copies every marked text, one per line. Both are in the Search menu too.
- **Search › Jump Down › Find Mark Style** (Alt+M) and **Jump Up** (Alt+Shift+M) select the next and previous mark (the keys work in the editor).
- While the Mark row is open, the find bar doesn't highlight the matches itself, so what you see is what Mark All marked.

So "keep only the lines that match" is: Mark with Bookmark line, then Search › Bookmark › Remove Non-Bookmarked Lines; one Ctrl+Z undoes it.

**Style tokens** (Search › Style All Occurrences of Token › Using 1st–5th Style) mark every occurrence of the selection, or of the word at the caret as a whole word, case-sensitively, in one of five colours. **Search › Clear Style** clears one style or all of them. **Search › Jump Down** (Ctrl+1 to Ctrl+5) and **Jump Up** (Ctrl+Alt+1 to Ctrl+Alt+5) go to the next and previous text in a style (the keys work in the editor). Marks and styles move as you edit; the session doesn't keep them.

## Split view

Two views side by side, each with its own tab strip.

- **View › Move/Clone Current Document › Move to Other View** moves the tab to the other view; **Clone to Other View** shows the same document there too: one text, one undo history, the same bookmarks and marks, but a caret and scroll position of its own in each view.
- **Focus on Another View** (Ctrl+Alt+O, F8) switches between them; clicking in a view does too. The view you are in is the active one: commands act there, new tabs open there, and the status bar and the window title follow it. Its tab is underlined in the theme's accent colour.
- Closing a clone never asks about unsaved changes: the document is still open in the other view. Closing the last tab of a view closes the split.
- **View › Synchronise Vertical Scrolling** and **Synchronise Horizontal Scrolling** scroll the other view along with the one you scroll.
- The session keeps the split: which view each tab is in, clones, the tab each view shows and where the divider is.

## Compare

**Tools › Compare › Compare** (Ctrl+Alt+C) compares the document you are in with the one in the other view; without a split, the tab you used before moves to the other view first. The document you are in is the new version, the other the old one. Also in Tools › Compare:

- **Compare with File…** (a file you choose), **Compare with Clipboard** (Ctrl+Alt+M) and **Compare with Saved Version** (Ctrl+Alt+D) open that text read-only in the other view; Clear Compare closes it again. The **Compare** button on the "changed on disk" banner, and on the banner of a restored document whose file changed while Stet was closed, compares your text with the file on disk.
- Lines only in the new version are added (green), lines only in the old one removed (red), versions of a line changed (blue, with the characters that differ in a stronger colour), and blocks that moved are moved (magenta); the colours are the theme's. Where one side has lines the other lacks, the other is padded, so equal lines face each other, and the two views scroll together. Word wrap is off while a document is compared.
- **Next Difference** (Alt+PgDn) and **Previous Difference** (Alt+PgUp) go round the differences.
- **Ignore Whitespace** and **Ignore Case** compare again without them.
- The status bar sums the comparison up: "12 lines added, 3 removed, 5 changed, 1 moved". Edit either document and the comparison follows a moment later. **Clear Compare** (Ctrl+Alt+X) ends it.

## Indentation

Every document has a tab width and either tabs or spaces, shown in the status bar as `Tab Width: 4` or `Spaces: 4`. Click it (or run **Indentation…** from the palette) for a popover: Spaces or Tabs, the width from 1 to 8, Detect from Content, and Convert Indentation to Spaces or to Tabs (the leading whitespace of every line, one undo step). A choice there sticks to that document.

Otherwise a file you open keeps the indentation it already uses (Stet reads its first 10,000 lines), and a new document gets the settings for its language: from `config.toml`, else Stet's defaults (Makefiles and Go use tabs; Python and Rust four spaces; YAML, JSON and XML two spaces), else tabs, four wide.

## Settings and keys

Stet's settings are two files in `$XDG_CONFIG_HOME/stet/` (usually `~/.config/stet/`). **Settings › Open Settings (config.toml)** and **Settings › Open Keyboard Shortcuts (keys.toml)** open them in a tab, creating them first with every default written out as a comment. Stet applies a file as soon as it is saved, from Stet or anywhere else. A file with a mistake is not applied at all: a notification gives the first mistake's line and column, and the previous settings stay until it is fixed.

### config.toml

| Setting | Default | |
| --- | --- | --- |
| `font` | Omarchy's monospace font | the editor's font family |
| `font_size` | `11` | points; zoom (Ctrl++, Ctrl+-) adds to it |
| `tab_width` | `4` | for new documents and undetected files |
| `insert_spaces` | `false` | `true`: Tab inserts spaces |
| `detect_indentation` | `true` | follow the indentation a file uses |
| `word_wrap`, `show_whitespace`, `document_map` | `false` | what the View menu starts with |
| `smart_highlight` | `true` | highlight the other places of a selected word |
| `word_count` | `false` | count words in the status bar |
| `backups`, `backup_interval` | `true`, `7` | backups of unsaved work, every N seconds (M2) |
| `highlight_max_bytes`, `highlight_max_lines` | `2097152`, `100000` | larger files open without syntax highlighting |

Per language, by GtkSourceView language id:

```toml
[language.python3]
tab_width = 4
insert_spaces = true
```

### keys.toml

Each line names a command and lists its keys, which replace its default keys; the first is the one menus show. `[]` leaves a command without a key:

```toml
uppercase = ["<Control><Shift>u", "<Alt><Shift>u"]
duplicate-line = ["<Alt>d"]
quit = []
```

Modifiers are `<Control>` (or `<Ctrl>`), `<Shift>`, `<Alt>` and `<Super>`; key names are GTK's (`a`–`z`, `0`–`9`, `F1`–`F35`, `Return`, `Tab`, `Up`, `Page_Down`, `plus`, `minus`, `KP_Add`, …). A key belongs to one command only: to give a command another one's key, unbind that one in the same file. A key needs Ctrl, Alt or Super unless it is an F-key. Undo, Redo, Cut, Copy, Paste, Delete and Select All keep the text view's own keys. **Help › Keyboard Shortcuts** shows the keys in use.

### UPPERCASE and fcitx5

fcitx5 (Omarchy's input method) takes Ctrl+Shift+U and Ctrl+Alt+Shift+U for Unicode entry before any app sees them, so Stet's UPPERCASE is **Alt+Shift+U**, not Ctrl+Shift+U. To use Ctrl+Shift+U, clear fcitx5's Unicode hotkeys ([INTEGRATIONS.md](INTEGRATIONS.md#6-fcitx5-key-grabs)) and add it in `keys.toml` as above.

## Keymap

Stet's default keys are adapted to Omarchy: every F-key command also has a key without an F-key, because the F-row often sends media keys (the menus show that key); Quit is Ctrl+Alt+Q (Ctrl+Q toggles comments); UPPERCASE is Alt+Shift+U. "Editor" keys work while the editor has the keyboard focus; "Window" keys anywhere in the window; "Text fields" keys are the text view's own and also work in the find bar. The table is generated from Stet's action registry (a test keeps it current).

<!-- keymap table: generated from the registry -->

| Menu | Command | Keys | Keys work | `keys.toml` name |
| --- | --- | --- | --- | --- |
| File | New | Ctrl+N | Window | `new-tab` |
| File | Open… | Ctrl+O | Window | `open` |
| File | Quick Open… | Ctrl+P | Window | `quick-open` |
| File › Open Recent | Clear Recent Files | — |  | `clear-recent` |
| File | Save | Ctrl+S | Window | `save` |
| File | Save As… | Ctrl+Alt+S | Window | `save-as` |
| File | Save All | Ctrl+Shift+S | Window | `save-all` |
| File | Close | Ctrl+W | Window | `close-tab` |
| File | Close All | Ctrl+Shift+W | Window | `close-all` |
| File | Restore Closed Tab | Ctrl+Shift+T | Window | `restore-closed-tab` |
| File | Forget Unsaved Drafts… | — |  | `forget-drafts` |
| File | Quit | Ctrl+Alt+Q, Alt+F4 | Window | `quit` |
| File | Reload from Disk | — |  | `reload-from-disk` |
| File | Rename… | — |  | `rename-file` |
| File | Move to Trash… | — |  | `move-to-trash` |
| File | Open Containing Folder | — |  | `open-containing-folder` |
| File | Open Terminal Here | — |  | `open-terminal-here` |
| File | Close Others | — |  | `close-others` |
| File | Close to the Right | — |  | `close-to-the-right` |
| Edit | Undo | Ctrl+Z | Text fields | — |
| Edit | Redo | Ctrl+Y, Ctrl+Shift+Z | Text fields | — |
| Edit | Cut | Ctrl+X, Shift+Del | Text fields | — |
| Edit | Copy | Ctrl+C, Ctrl+Ins | Text fields | — |
| Edit | Paste | Ctrl+V, Shift+Ins | Text fields | — |
| Edit | Delete | Del | Text fields | — |
| Edit | Select All | Ctrl+A | Text fields | — |
| Edit › EOL Conversion | Windows (CR LF) | — |  | `eol-crlf` |
| Edit › EOL Conversion | Unix (LF) | — |  | `eol-lf` |
| Edit › EOL Conversion | Macintosh (CR) | — |  | `eol-cr` |
| Edit › Copy to Clipboard | Copy Full Path | — |  | `copy-full-path` |
| Edit › Copy to Clipboard | Copy File Name | — |  | `copy-file-name` |
| Edit › Copy to Clipboard | Copy Directory Path | — |  | `copy-directory-path` |
| Edit › Convert Case to | UPPERCASE | Alt+Shift+U | Editor | `uppercase` |
| Edit › Convert Case to | lowercase | Ctrl+U | Editor | `lowercase` |
| Edit › Convert Case to | Proper Case | Alt+U | Editor | `proper-case` |
| Edit › Convert Case to | Proper Case (blend) | — |  | `proper-case-blend` |
| Edit › Convert Case to | Sentence case | Ctrl+Alt+U | Editor | `sentence-case` |
| Edit › Convert Case to | Sentence case (blend) | — |  | `sentence-case-blend` |
| Edit › Convert Case to | iNVERT cASE | — |  | `invert-case` |
| Edit › Line Operations | Duplicate Current Line | Ctrl+D | Editor | `duplicate-line` |
| Edit › Line Operations | Cut Current Line | Ctrl+L | Editor | `cut-line` |
| Edit › Line Operations | Copy Current Line | Ctrl+Shift+X | Editor | `copy-line` |
| Edit › Line Operations | Delete Current Line | Ctrl+Shift+L | Editor | `delete-line` |
| Edit › Line Operations | Transpose Current Line | Ctrl+T | Editor | `transpose-line` |
| Edit › Line Operations | Move Up Current Line | Ctrl+Shift+↑ | Editor | `move-line-up` |
| Edit › Line Operations | Move Down Current Line | Ctrl+Shift+↓ | Editor | `move-line-down` |
| Edit › Line Operations | Join Lines | Ctrl+J | Editor | `join-lines` |
| Edit › Line Operations | Insert Blank Line Above Current | Ctrl+Alt+Enter | Editor | `blank-line-above` |
| Edit › Line Operations | Insert Blank Line Below Current | Ctrl+Alt+Shift+Enter | Editor | `blank-line-below` |
| Edit › Line Operations | Remove Duplicate Lines | — |  | `remove-duplicate-lines` |
| Edit › Line Operations | Remove Consecutive Duplicate Lines | — |  | `remove-consecutive-duplicate-lines` |
| Edit › Line Operations | Remove Empty Lines | — |  | `remove-empty-lines` |
| Edit › Line Operations | Remove Empty Lines (Containing Blank Characters) | — |  | `remove-blank-lines` |
| Edit › Line Operations | Reverse Line Order | — |  | `reverse-lines` |
| Edit › Line Operations | Sort Lines Lexicographically Ascending | — |  | `sort-lexical-ascending` |
| Edit › Line Operations | Sort Lines Lexicographically Descending | — |  | `sort-lexical-descending` |
| Edit › Line Operations | Sort Lines Lex. Ascending Ignoring Case | — |  | `sort-ignore-case-ascending` |
| Edit › Line Operations | Sort Lines Lex. Descending Ignoring Case | — |  | `sort-ignore-case-descending` |
| Edit › Line Operations | Sort Lines As Integers Ascending | — |  | `sort-integer-ascending` |
| Edit › Line Operations | Sort Lines As Integers Descending | — |  | `sort-integer-descending` |
| Edit › Line Operations | Sort Lines As Decimals (Comma) Ascending | — |  | `sort-decimal-comma-ascending` |
| Edit › Line Operations | Sort Lines As Decimals (Comma) Descending | — |  | `sort-decimal-comma-descending` |
| Edit › Line Operations | Sort Lines As Decimals (Dot) Ascending | — |  | `sort-decimal-dot-ascending` |
| Edit › Line Operations | Sort Lines As Decimals (Dot) Descending | — |  | `sort-decimal-dot-descending` |
| Edit › Line Operations | Sort Lines By Length Ascending | — |  | `sort-length-ascending` |
| Edit › Line Operations | Sort Lines By Length Descending | — |  | `sort-length-descending` |
| Edit › Comment/Uncomment | Toggle Single Line Comment | Ctrl+Q | Editor | `toggle-comment` |
| Edit › Comment/Uncomment | Single Line Comment | Ctrl+K | Editor | `comment-lines` |
| Edit › Comment/Uncomment | Single Line Uncomment | Ctrl+Shift+K | Editor | `uncomment-lines` |
| Edit › Comment/Uncomment | Block Comment | Ctrl+Shift+Q | Editor | `toggle-block-comment` |
| Edit › Blank Operations | Trim Trailing Space | — |  | `trim-trailing` |
| Edit › Blank Operations | Trim Leading Space | — |  | `trim-leading` |
| Edit › Blank Operations | Trim Leading and Trailing Space | — |  | `trim-both` |
| Edit › Blank Operations | TAB to Space | — |  | `tabs-to-spaces` |
| Edit › Blank Operations | Space to TAB (All) | — |  | `spaces-to-tabs` |
| Edit › Blank Operations | Space to TAB (Leading) | — |  | `spaces-to-tabs-leading` |
| Edit | Add Prefix/Suffix… | — |  | `add-prefix-suffix` |
| Edit | Insert Numbers… | — |  | `insert-numbers` |
| Edit | Begin/End Select in Column Mode | Alt+Shift+B | Window | `column-begin-end-select` |
| Edit | Column Editor… | Alt+C | Window | `column-editor` |
| Search | Find… | Ctrl+F | Window | `find` |
| Search | Find in Files… | Ctrl+Shift+F | Window | `find-in-files` |
| Search | Find Next | Alt+↓, F3 | Window | `find-next` |
| Search | Find Previous | Alt+↑, Shift+F3 | Window | `find-previous` |
| Search | Replace… | Ctrl+H | Window | `replace` |
| Search | Search Results Window | Ctrl+Alt+R, F7 | Window | `search-results` |
| Search | Next Search Result | Ctrl+Alt+↓, F4 | Window | `next-search-result` |
| Search | Previous Search Result | Ctrl+Alt+↑, Shift+F4 | Window | `previous-search-result` |
| Search | Go to Line… | Ctrl+G | Window | `go-to-line` |
| Search | Go Back | Alt+← | Window | `go-back` |
| Search | Go Forward | Alt+→ | Window | `go-forward` |
| Search | Go to Matching Brace | Ctrl+B | Editor | `go-to-matching-brace` |
| Search | Select to Matching Brace | Ctrl+Alt+B | Editor | `select-to-matching-brace` |
| Search | Select and Find Next | Ctrl+Alt+F, Ctrl+F3 | Editor | `select-and-find-next` |
| Search | Select and Find Previous | Ctrl+Alt+Shift+F, Ctrl+Shift+F3 | Editor | `select-and-find-previous` |
| Search › Bookmark | Toggle Bookmark | Ctrl+Alt+K, Ctrl+F2 | Window | `toggle-bookmark` |
| Search › Bookmark | Next Bookmark | Ctrl+Alt+L, F2 | Window | `next-bookmark` |
| Search › Bookmark | Previous Bookmark | Ctrl+Alt+J, Shift+F2 | Window | `previous-bookmark` |
| Search › Bookmark | Clear All Bookmarks | — |  | `clear-bookmarks` |
| Search › Bookmark | Cut Bookmarked Lines | — |  | `cut-bookmarked-lines` |
| Search › Bookmark | Copy Bookmarked Lines | — |  | `copy-bookmarked-lines` |
| Search › Bookmark | Paste to (Replace) Bookmarked Lines | — |  | `paste-to-bookmarked-lines` |
| Search › Bookmark | Remove Bookmarked Lines | — |  | `remove-bookmarked-lines` |
| Search › Bookmark | Remove Non-Bookmarked Lines | — |  | `remove-unbookmarked-lines` |
| Search › Bookmark | Inverse Bookmarks | — |  | `inverse-bookmarks` |
| Search | Mark… | Ctrl+M | Window | `mark` |
| Search | Clear All Marks | — |  | `clear-marks` |
| Search | Copy Marked Text | — |  | `copy-marked-text` |
| Search › Style All Occurrences of Token | Using 1st Style | — |  | `style-token-1` |
| Search › Style All Occurrences of Token | Using 2nd Style | — |  | `style-token-2` |
| Search › Style All Occurrences of Token | Using 3rd Style | — |  | `style-token-3` |
| Search › Style All Occurrences of Token | Using 4th Style | — |  | `style-token-4` |
| Search › Style All Occurrences of Token | Using 5th Style | — |  | `style-token-5` |
| Search › Clear Style | Clear 1st Style | — |  | `clear-style-1` |
| Search › Clear Style | Clear 2nd Style | — |  | `clear-style-2` |
| Search › Clear Style | Clear 3rd Style | — |  | `clear-style-3` |
| Search › Clear Style | Clear 4th Style | — |  | `clear-style-4` |
| Search › Clear Style | Clear 5th Style | — |  | `clear-style-5` |
| Search › Clear Style | Clear All Styles | — |  | `clear-all-styles` |
| Search › Jump Up | 1st Style | Ctrl+Alt+1 | Editor | `jump-up-1` |
| Search › Jump Up | 2nd Style | Ctrl+Alt+2 | Editor | `jump-up-2` |
| Search › Jump Up | 3rd Style | Ctrl+Alt+3 | Editor | `jump-up-3` |
| Search › Jump Up | 4th Style | Ctrl+Alt+4 | Editor | `jump-up-4` |
| Search › Jump Up | 5th Style | Ctrl+Alt+5 | Editor | `jump-up-5` |
| Search › Jump Up | Find Mark Style | Alt+Shift+M | Editor | `jump-up-mark` |
| Search › Jump Down | 1st Style | Ctrl+1 | Editor | `jump-down-1` |
| Search › Jump Down | 2nd Style | Ctrl+2 | Editor | `jump-down-2` |
| Search › Jump Down | 3rd Style | Ctrl+3 | Editor | `jump-down-3` |
| Search › Jump Down | 4th Style | Ctrl+4 | Editor | `jump-down-4` |
| Search › Jump Down | 5th Style | Ctrl+5 | Editor | `jump-down-5` |
| Search › Jump Down | Find Mark Style | Alt+M | Editor | `jump-down-mark` |
| View | Word Wrap | — |  | `word-wrap` |
| View | Show Whitespace | — |  | `show-whitespace` |
| View | Zoom In | Ctrl++, Ctrl+=, Ctrl+Num + | Window | `zoom-in` |
| View | Zoom Out | Ctrl+-, Ctrl+Num - | Window | `zoom-out` |
| View | Reset Zoom | Ctrl+0, Ctrl+Num / | Window | `zoom-reset` |
| View | Full Screen | Alt+Enter, F11 | Window | `full-screen` |
| View | Next Tab | Ctrl+PgDn | Window | `next-tab` |
| View | Previous Tab | Ctrl+PgUp | Window | `previous-tab` |
| View | Next Recent Tab | Ctrl+Tab | Window | `next-recent-tab` |
| View | Previous Recent Tab | Ctrl+Shift+Tab | Window | `previous-recent-tab` |
| View | Move Tab Forward | Ctrl+Shift+PgDn | Window | `move-tab-forward` |
| View | Move Tab Backward | Ctrl+Shift+PgUp | Window | `move-tab-backward` |
| View | Pin Tab | — |  | `pin-tab` |
| View | Unpin Tab | — |  | `unpin-tab` |
| View | Command Palette… | Ctrl+Shift+P, F1 | Window | `command-palette` |
| View | Document Map | — |  | `document-map` |
| View › Move/Clone Current Document | Move to Other View | — |  | `move-to-other-view` |
| View › Move/Clone Current Document | Clone to Other View | — |  | `clone-to-other-view` |
| View | Focus on Another View | Ctrl+Alt+O, F8 | Window | `switch-view` |
| View | Synchronise Vertical Scrolling | — |  | `sync-vertical-scrolling` |
| View | Synchronise Horizontal Scrolling | — |  | `sync-horizontal-scrolling` |
| Encoding | Encode in UTF-8 | — |  | `encode-utf8` |
| Encoding | Encode in UTF-8-BOM | — |  | `encode-utf8-bom` |
| Encoding | Encode in UTF-16 BE BOM | — |  | `encode-utf16be-bom` |
| Encoding | Encode in UTF-16 LE BOM | — |  | `encode-utf16le-bom` |
| Encoding | Convert to ANSI (Windows-1252) | — |  | `convert-to-ansi` |
| Encoding | Convert to UTF-8 | — |  | `convert-to-utf8` |
| Encoding | Convert to UTF-8-BOM | — |  | `convert-to-utf8-bom` |
| Encoding | Convert to UTF-16 BE BOM | — |  | `convert-to-utf16be-bom` |
| Encoding | Convert to UTF-16 LE BOM | — |  | `convert-to-utf16le-bom` |
| Language | Set Language… | — |  | `choose-language` |
| Settings | Open Settings (config.toml) | — |  | `open-settings` |
| Settings | Open Keyboard Shortcuts (keys.toml) | — |  | `open-keyboard-shortcuts` |
| Settings | Set as Default Editor… | — |  | `set-as-default-editor` |
| Tools › JSON | Format JSON | Ctrl+Alt+Shift+M | Editor | `format-json` |
| Tools › JSON | Minify JSON | Ctrl+Alt+Shift+C | Editor | `minify-json` |
| Tools › JSON | Validate JSON | Ctrl+Alt+Shift+J | Editor | `validate-json` |
| Tools › XML | Format XML | Ctrl+Alt+Shift+B | Editor | `format-xml` |
| Tools › XML | Validate XML | Ctrl+Alt+Shift+X | Editor | `validate-xml` |
| Tools › Compare | Compare | Ctrl+Alt+C | Window | `compare` |
| Tools › Compare | Compare with File… | — |  | `compare-with-file` |
| Tools › Compare | Compare with Clipboard | Ctrl+Alt+M | Window | `compare-with-clipboard` |
| Tools › Compare | Compare with Saved Version | Ctrl+Alt+D | Window | `compare-with-saved` |
| Tools › Compare | Previous Difference | Alt+PgUp | Window | `previous-difference` |
| Tools › Compare | Next Difference | Alt+PgDn | Window | `next-difference` |
| Tools › Compare | Ignore Whitespace | — |  | `compare-ignore-whitespace` |
| Tools › Compare | Ignore Case | — |  | `compare-ignore-case` |
| Tools › Compare | Clear Compare | Ctrl+Alt+X | Window | `clear-compare` |
| Help | Keyboard Shortcuts | — |  | `keyboard-shortcuts` |
| Help | About Stet | — |  | `about` |
| Palette | Replace Next | — |  | `replace-next` |
| Palette | Replace All | — |  | `replace-all` |
| Palette | Replace All in Open Documents | — |  | `replace-all-in-open-documents` |
| Palette | Count | — |  | `count` |
| Palette | Find All in Current Document | — |  | `find-all-in-document` |
| Palette | Find All in Open Documents | — |  | `find-all-in-open-documents` |
| Palette | Stop Find in Files | — |  | `stop-find-in-files` |
| Palette | Replace in Files | — |  | `replace-in-files` |
| Palette | Copy Search Results | — |  | `copy-search-results` |
| Palette | Clear Search Results | — |  | `clear-search-results` |
| Palette | Close Search Results | — |  | `close-search-results` |
| Palette | Encoding… | — |  | `choose-encoding` |
| Palette | Indentation… | — |  | `choose-indentation` |
| Palette | Detect Indentation from Content | — |  | `detect-indentation` |
| Palette | Column Select Left | Alt+Shift+←, Alt+Shift+Num ← | Window | `column-select-left` |
| Palette | Column Select Right | Alt+Shift+→, Alt+Shift+Num → | Window | `column-select-right` |
| Palette | Column Select Up | Alt+Shift+↑, Alt+Shift+Num ↑ | Window | `column-select-up` |
| Palette | Column Select Down | Alt+Shift+↓, Alt+Shift+Num ↓ | Window | `column-select-down` |
| Palette | Column Select to Line Start | Alt+Shift+Home, Alt+Shift+Num Home | Window | `column-select-line-start` |
| Palette | Column Select to Line End | Alt+Shift+End, Alt+Shift+Num End | Window | `column-select-line-end` |
| Palette | Column Select Page Up | Alt+Shift+PgUp, Alt+Shift+Num PgUp | Window | `column-select-page-up` |
| Palette | Column Select Page Down | Alt+Shift+PgDn, Alt+Shift+Num PgDn | Window | `column-select-page-down` |
| Palette | Mark All | — |  | `mark-all` |

<!-- end of the keymap table -->

## Omarchy

- **Theme and font**: Stet follows the Omarchy theme (`omarchy theme set`) and font (`omarchy-font-set`) live; `font` and `font_size` in `config.toml` override the font.
- **Set as default editor**: **Settings › Set as Default Editor…** makes SUPER+SHIFT+N (`omarchy-launch-editor`) start Stet and makes Stet the default for the text types in its desktop entry, so a double-click in the file manager opens Stet. It says exactly what it will change, asks first, and reports what changed; [INTEGRATIONS.md](INTEGRATIONS.md#4-set-as-default-editor-built-in-m5) has the details and how to undo it.
- **Terminal and folder**: Open Terminal Here uses `xdg-terminal-exec --dir=<folder>`, as Omarchy's own terminal launcher does; Open Containing Folder uses `xdg-open`.
- `GIT_EDITOR="stet --wait"` and `SUDO_EDITOR="stet --wait"` arrive with M2.
