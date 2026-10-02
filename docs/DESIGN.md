# Design & Interaction System

Project: Stet, a fast, keyboard-first text and code editor for Omarchy

Synced: 2026-10-02

The repository docs are the canonical project memory ([ADR-012](DECISIONS.md)). Dated entries describe the state at that date; newer entries take precedence.

---

## As built in 1.3.2 — 2026-10-02

Asked for by the user after 1.3.1 ([PLAN.md](PLAN.md#stet-132-a-narrower-menu-about-and-the-icon--2026-10-02)):

- **The menu's width:** each page of the hamburger menu is as wide as its own items. GTK's popover menu keeps its pages (the main one and one per submenu) in a stack that is as wide as its widest page; Stet makes that stack's width follow the page it shows (`hhomogeneous` off), so the main page is a narrow column and a submenu widens the menu as it slides in (GTK animates the size). The popover still opens from the button's left edge (1.3.1), so it grows to the right, into the window. Its height is limited to the room between the button and the window's bottom edge (less 40 px for the arrow, border and a margin, at least 160 px), set each time it opens: a page taller than that, such as Search or Edit › Line Operations in a short window, scrolls instead of hanging below the window.
- **About** (Help › About Stet, an AdwAboutDialog): Details says what Stet is and does in three short paragraphs; Website and Report an Issue point to the GitHub repository (private for now, so they open only for accounts with access); Legal has "© 2026 Stet contributors" and the MIT license.
- **The icon** is the note alone: the page with its lines, the red strike and the green dots, 1.2 times its old size so that it fills the 128 grid (86.4 × 110.4 at 20.8, 8.8), with a thin dark edge at 22 % opacity so it keeps its outline on light backgrounds. The dark rounded tile it sat on is gone. The symbolic icon was already the page alone.
- **1.3.3:** `stet --help`, the package description and the desktop entry's comment are reworded; the comment reads "Edit text and code files, keyboard-first".

## As built in 1.3 — 2026-10-01

Asked for by the user after 1.2 ([PLAN.md](PLAN.md#stet-130-the-menu-on-the-left--2026-10-01)):

- **The hamburger menu** is the start action widget of the first tab strip on screen, left of the tabs: the main view's, or the second view's while the main view has no tab. It was at the end of the last strip until 1.2. Its popover opens below it as before.
- **Outside Hyprland,** the window controls follow the decoration layout: those it puts first sit before the menu, the others at the end of the last strip, as before.
- A click on the menu or the window controls doesn't make their strip's view the active one; a double-click there is no double-click on a tab.
- **1.3.1:** the menu's popover opens from the button's left edge into the window (`halign` start). Centred on the button, as in 1.3.0, half of it hung outside the window's left edge whenever the window didn't start at the screen's edge, a floating window or one tiled on the right.

```
+------------------------------------------------------------------------------+
| [=] | main.rs  x | ● notes.txt  x |              | Cargo.toml  x |           |  split: the menu starts the
+------------------------------------------------------------------------------+  first strip on screen
```

## As built in 1.2 — 2026-10-01

Asked for by the user after 1.1 ([PLAN.md](PLAN.md#stet-120-names-for-untitled-tabs-without-saving--2026-10-01)): tabs can be named without saving.

- **Rename…** (formerly Rename File…; tab menu, File menu, palette, a double-click on the tab) renames a file on disk as before. On an untitled document it gives the tab a name and saves nothing: the dialog is the same, titled "Rename “name”", with the current name selected and the body "Only the tab's name changes; nothing is saved, and Save As… proposes the name. Leave it empty to name the tab after its first line again." Line breaks and other control characters become spaces; the name is trimmed. Pressing Enter on the unchanged first-line name keeps the tab following its first line.
- **The name** replaces the first-line name (M8) in the tab, its tooltip, the window title, the Ctrl+Tab switcher and everything else that shows the document's name, in every view of it (clones). Save As… proposes it as the file name (made safe, with the language's extension or `.txt`); once saved, the tab shows the file's name.
- **Kept:** in the session (`custom_name`, [ADR-006 amendment](DECISIONS.md#adr-006-amendment--names-for-untitled-tabs-12-2026-10-01)), also for a restored tab whose text isn't loaded yet, and by Restore Closed Tab.
- The double-click on an untitled tab, Save As… in 1.1, opens Rename… now.

## As built in 1.1 — 2026-10-01

Two additions the user asked for after 1.0 ([PLAN.md](PLAN.md#stet-110-double-click-a-tab-to-rename-save-in-the-tab-menu--2026-10-01)):

- **A double-click on a tab** opens Rename File… for its file: the tab menu's dialog, with the name before its extension selected. On an untitled tab it opens Save As…, which is how an untitled document gets a name. A tab still loading and a text a comparison opened ignore it. Both presses must land on the same tab and not on its buttons: a quick second click on a close button lands on the tab that moves in under it, and must not rename that one.
- **How:** a click gesture on each tab strip (AdwTabBar) in the capture phase, so it sees a press before libadwaita's own handling; the tab under the pointer is libadwaita's internal `AdwTab` widget, read through its `page` property. If a libadwaita release renames that widget, a double-click becomes two plain clicks and `tab-double-click.stet-test` fails. The second press is claimed, so outside Hyprland the window handle around the strips doesn't also maximise the window.
- **Tab context menu:** Pin Tab or Unpin Tab | Close, Close Others, Close to the Right | **Save, Save As…** | Copy Full Path, Copy File Name, Copy Directory Path | Open Containing Folder, Open Terminal Here | Rename File…, Move to Trash…. Save and Save As… come after the close commands. Like every item they act on the tab that was clicked; with no menu open, Save and Save As… act on the current tab as before.

## As built in M7 — 2026-10-01

Bookmarks, Mark, split view and compare, built under the standing authorization of 2026-10-01 on top of M8's merge. Headless screenshots: [m7-bookmarks-marks-tokyo-night](../tools/progress/public/m7-bookmarks-marks-tokyo-night-2026-10-01.png), [m7-split-clone-catppuccin-latte](../tools/progress/public/m7-split-clone-catppuccin-latte-2026-10-01.png), [m7-compare-gruvbox](../tools/progress/public/m7-compare-gruvbox-2026-10-01.png). It adds to the entries below; the presentation decisions are [ADR-018](DECISIONS.md#adr-018--split-view-and-compare-presentation-m7-2026-10-01).

```
+--------------------------------------+---------------------------------------+
| ● new.txt x | main.rs x              | old.txt x | [=]                       |  one strip per view; the
+--------------------------------------+---------------------------------------+  active view's tab underlined
| 1   header line one                  | 1   header line one                   |
| 2   keep line one                    | 2   keep line one                     |
|     ~~~~~~~~~~~~~~~~ (padding)       | 3   this line is removed       (red)  |
| 3   keep line two                    | 4   keep line two                     |
| 4   the quick [red] fox     (blue)   | 5   the quick [brown] fox      (blue) |  changed: the words differ
| …                                    | …                                     |
+--------------------------------------+---------------------------------------+
| [Find________] Aa W Sel [Regex v] ^ v  Mark: 3 matches                ⋮  x   |  find bar, Mark row (Ctrl+M):
| [Find Mark Style v] [x] Bookmark line [ ] Purge   Mark All  Clear  Copy      |  style, options, buttons
+------------------------------------------------------------------------------+
| Ln 3, Col 1 | 13 lines | 1 line added, 1 removed, 1 changed, 2 moved | …     |  the comparison's summary
+------------------------------------------------------------------------------+
```

- **Bookmarks** are a disc in the theme's accent colour (scheme style `stet:bookmark`, foreground) in the gutter's mark column, left of the line numbers; GtkSourceView draws it from a picture Stet paints in that colour, again on every theme change. A click in the mark column (any line) toggles a bookmark; the column is always there, so the text never shifts.
- **Mark and the token styles** colour the text's background: the Find Mark Style is `red` and the five token styles `cyan`, `orange`, `yellow`, `magenta` and `green`, each 40 % into `background` (scheme styles `stet:mark`, `stet:token-1` to `-5`), as text tags around the lines on screen (100 lines above and below each view), recoloured on theme change. They don't change the text's colour, so syntax highlighting stays readable.
- **The Mark row** of the find bar (Search › Mark…, Ctrl+M) replaces the replace and files rows: a style drop-down (Find Mark Style, 1st–5th Style), Bookmark line, Purge for each search, then Mark All (the suggested action), Clear All Marks and Copy Marked Text. The find row stays as it is; its message says "Mark: 12 matches" (or "no matches"), and Enter in the find field marks. While the Mark row shows, the bar's own highlighting of every match is off, so the marks show what Mark All found.
- **Split view:** the two views sit side by side under their own tab strips, divided at the same place by one thin handle (two GtkPaned bound to one position); the menu button sits at the end of the right-hand strip. A view shows only while it has tabs, so one view looks exactly as before M7. While split, the active view's selected tab is underlined with a 2 px bar in `--accent-bg-color`; the other view's selected tab keeps libadwaita's plain selected style. Both views show the same document's selection and current-line highlight when they show the same document (one buffer); each keeps its own caret position and scroll position.
- **Compare** colours whole lines with paragraph backgrounds (the view paints empty lines, which GTK leaves out), 20 % of a palette colour into `background`: added `green`, removed `red`, changed `blue`, moved `magenta` (scheme styles `stet:compare-added`, `-removed`, `-changed`, `-moved`); the characters that differ inside a changed line get `blue` at 45 % (`stet:compare-changed-text`). Where the other side has lines this one lacks, an empty band of that many lines' height (`muted` at 25 %, `stet:compare-padding`) keeps equal lines facing each other: below the line before the gap (`pixels-below-lines`), or as the view's top or bottom margin at either end. The gutter's line numbers and bookmarks sit at the top of their line, beside its text rather than in the middle of a padded line. Word wrap is off in compared views, the views scroll level, and the status bar shows the summary ("12 lines added, 3 removed, 5 changed, 1 moved"; "No differences"), also as a toast when the comparison first shows. Texts the comparison opens (the clipboard, a chosen file, the saved version, named "Clipboard", the file's name, "name (saved)") are read-only tabs in the other view.
- **The changed-on-disk banner** has Compare first: "“a.txt” changed on disk." [Compare] [Reload] [Keep Mine]; the restored-conflict banner: [Compare] [Keep Mine] [Load Disk Version].
- **Menus:** Search gains Mark…, Clear All Marks, Copy Marked Text, and the submenus Style All Occurrences of Token, Clear Style, Jump Up, Jump Down and Bookmark, in that order; View gains Move/Clone Current Document (Move to Other View, Clone to Other View), Focus on Another View and the two Synchronise Scrolling toggles; Tools gains Compare (Compare, Compare with File…, Compare with Clipboard, Compare with Saved Version | Previous and Next Difference | Ignore Whitespace, Ignore Case | Clear Compare).

## As built in M8 — 2026-10-01

Headless screenshots: [m8-quick-open-tokyo-night](../tools/progress/public/m8-quick-open-tokyo-night-2026-10-01.png), [m8-switcher-pinned-catppuccin-latte](../tools/progress/public/m8-switcher-pinned-catppuccin-latte-2026-10-01.png), [m8-replace-in-files-confirm-gruvbox](../tools/progress/public/m8-replace-in-files-confirm-gruvbox-2026-10-01.png), [m8-replace-in-files-results-gruvbox](../tools/progress/public/m8-replace-in-files-results-gruvbox-2026-10-01.png). It adds to the entries below.

```
                +--------------------------------------------------------+
                | [gre_____________________________________________]     |  quick open (Ctrl+P),
                | ~/proj/src/**gre**et.rs                          open  |  top centre, 640 px
                | src/**gr**amm**a**r.rs                                 |  matched characters bold;
                | notes/**gre**etings.md                         recent  |  open / recent / project
                | ~/proj: 1,204 files                                    |  what the walk found
                +--------------------------------------------------------+

                         +-----------------------------------+
                         | Switch to                         |  Ctrl+Tab held: centred,
                         |   main.rs        ~/proj/src       |  most recently used first,
                         | > ● notes.md     ~/proj           |  the selection highlighted
                         |   Shopping list                   |
                         +-----------------------------------+
```

- **Quick open** (Ctrl+P) reuses the palette's popover at the top centre of the editor area, 640 px wide: a field, up to 50 rows and a note. Each row is the candidate (open documents and recent files by their full path with `~`, project files relative to the project folder; the start is ellipsized) with the matched characters in bold, and "open" or "recent" on the right for those. Without a query the open documents come first, most recently used first, then the recent files, then the project's files. "No file matches" when nothing does; `:42` shows a single row "Go to line 42". The note under the list says what the walk found: "Listing the files in ~/proj… 800 so far", "~/proj: 1,204 files", or "~/proj: the first 100,000 files (more are left out)". Up and Down move, Enter opens, Escape closes; the editor gets the focus back either way.
- **Ctrl+Tab switcher:** while Ctrl is held after a Ctrl+Tab, a card in the middle of the window ("Switch to", then the tabs in most-recently-used order: the tab's name with its dirty dot, its folder dimmed) appears after 120 ms; the selected tab is highlighted. It never takes the focus or the pointer. A quick tap switches without showing it.
- **Pinned tabs** sit at the start of the strip, icon-only (the file type's icon, a dot while there are unsaved changes; libadwaita's pinned-tab style), in the order they were pinned. The tab strip's context menu (a right click on a tab) offers Pin Tab or Unpin Tab, whichever applies, and Close; the View menu and the palette have both, enabled as they apply to the current tab.
- **Untitled tabs** are named after their first line with text ("Shopping list"), white space folded, cut at 30 characters with "…"; without text "Untitled N". The window title, the switcher, quick open, the close prompt and Save As (which proposes "Shopping list.txt", or the language's extension) follow it.
- **Back and forward** have no chrome of their own: Search › Go Back and Go Forward (Alt+Left, Alt+Right) and the mouse's side buttons.
- **Replace in Files:** in Find in Files mode the find bar shows the replace row's field too, and the files row gets **Replace in Files** next to Find in Files; the document's Replace buttons stay in the Replace row. Replace in Files first asks in a destructive dialog: "Replace in Files?" — "Every match of “TODO” is replaced with “DONE” in the files matching \*.rs in ~/proj (with subfolders, hidden files left out, what .gitignore ignores left out). Files on disk are changed in place, and that can't be undone. Open documents get one edit each, which Undo takes back." — Cancel (default) or Replace in Files. While it runs the panel's status says "Replacing in ~/proj… 120 files searched, 14 changed" with Stop. The results heading is "Replace “TODO” with “DONE” (12 replacements in 5 files of 120 searched; 1 skipped)", and each file row says what happened: "(3 replacements)", "(2 replacements in the open document, saved)", "(1 replacement in the open document, which has unsaved changes)", "(1 match not replaced: binary file)", "(2 matches not replaced: Shift_JIS can't represent 1 character of the replacement (✓))"; the replaced lines follow, as Find in Files lists hits.

## As built in M6 — 2026-10-01

Column mode, built under the standing authorization of 2026-10-01 on top of Wave A. Headless screenshots: [m6-block-tokyo-night](../tools/progress/public/m6-block-tokyo-night-2026-10-01.png) (a block across tabs and into virtual space), [m6-caret-catppuccin-latte](../tools/progress/public/m6-caret-catppuccin-latte-2026-10-01.png) (a column caret after typing), [m6-column-editor-gruvbox](../tools/progress/public/m6-column-editor-gruvbox-2026-10-01.png) and [m6-numbers-gruvbox](../tools/progress/public/m6-numbers-gruvbox-2026-10-01.png) (the Column Editor and the numbers it wrote). It adds to the entries below.

- **The rectangle** is painted by the editor below the text: the scheme's `selection` background at 85 % opacity over the block's cells, and a 2 px bar in the scheme's `cursor` colour at the cursor corner, on every line of a column caret and on the corner's line of a block. GTK's own caret is hidden meanwhile. Only the visible lines are painted. The edges stay on the character grid across tabs (exact tab stops, ADR-015) and in virtual space past the end of a line; on lines with wide characters they follow the glyphs. The colours follow the theme on the next frame.
- **Status bar:** in column mode `Ln, Col` follow the cursor corner, virtual columns included, and `Sel` shows rows × columns: `Ln 7, Col 22, Sel 6×16`; a column caret on six lines is `Sel 6×0`.
- **Column Editor** (Alt+C, Edit › Column Editor…, the palette): an in-window dialog (AdwDialog, 520 px wide, a header with its title and close button). "Text to insert" with its field, or "Number to insert" with Initial number, Increase by, Repeat, Leading (None, Zeros, Spaces) and Format (Dec, Hex, Oct, Bin, and Uppercase for Hex); what doesn't apply is greyed out. Enter in a field or OK writes the column and closes the dialog; an error ("“Initial number” must be a decimal number.", "The text must fit on one line.", "The numbers do not fit in 64 bits.", "This document is read-only.") shows in the error colour under the fields and the dialog stays open; Escape, Cancel or the close button close it. The keyboard focus starts in the first field and goes back to the editor. The values are kept for the next use while Stet runs. In a rectangle the written column stays selected; with only a caret the caret stays where the text begins.
- **Menu and palette:** Edit has Begin/End Select in Column Mode (Alt+Shift+B) and Column Editor… (Alt+C); the eight Column Select commands are in the palette with their keys, not in the menu.
- **Mouse:** Alt+drag selects a rectangle, and the view follows the pointer while it moves; Alt+click puts a column caret; Alt+Shift+click moves the cursor corner; a plain click leaves column mode.

## As built in M5 — 2026-10-01

Built under the standing authorization of 2026-10-01; headless screenshots: [m5-status-and-map-tokyo-night](../tools/progress/public/m5-status-and-map-tokyo-night-2026-10-01.png), [m5-insert-numbers-catppuccin-latte](../tools/progress/public/m5-insert-numbers-catppuccin-latte-2026-10-01.png), [m5-indentation-gruvbox](../tools/progress/public/m5-indentation-gruvbox-2026-10-01.png), [m5-context-menu-gruvbox](../tools/progress/public/m5-context-menu-gruvbox-2026-10-01.png), [m5-shortcuts-tokyo-night](../tools/progress/public/m5-shortcuts-tokyo-night-2026-10-01.png). Where this differs from the entries below or the proposal, this entry wins.

- **Status bar, left to right:** `Ln 5, Col 1, Sel 36 | 2` (characters and the lines the selection touches; `Sel 0` without one), the document's `309 lines, 6,098 chars`, the word count when `word_count` is on (`968 words`, counted 250 ms after the text rests, not in large-file mode), then the buttons: language, line ending, encoding, **indentation** (`Spaces: 4` or `Tab Width: 4`), then `INS`/`OVR`.
- **Indentation popover** (from its button or the palette's Indentation…): "Indent with" Spaces | Tabs, "Tab width" 1–8 as linked toggle buttons, then Detect from Content, Convert Indentation to Spaces, Convert Indentation to Tabs. A toggle applies at once and the popover stays; the buttons apply and close it. The focus starts on the active Spaces/Tabs toggle and returns to the editor when the popover closes.
- **Document map:** GtkSourceMap at the editor's right edge, View › Document Map (a check item), off by default (`document_map` in config.toml), hidden in large-file mode.
- **Menu:** File, Edit, Search, View, Encoding, Language, **Settings**, **Tools**, Help. Edit has the submenus Copy to Clipboard, Convert Case to, Line Operations (four sections: the current line, moving and joining, removals and Reverse, the twelve sorts), Comment/Uncomment, EOL Conversion and Blank Operations, then Add Prefix/Suffix… and Insert Numbers…; the order of nested submenus is fixed in `SUBMENU_ORDER`. File gains Rename File…, Open Containing Folder, Open Terminal Here, Close Others, Close to the Right and Move to Trash…; Search gains Select and Find Next and Previous, Go to and Select to Matching Brace; Settings holds Open Settings (config.toml), Open Keyboard Shortcuts (keys.toml) and Set as Default Editor…; Tools holds JSON and XML; Help holds Keyboard Shortcuts. Editor keys are shown through the menu items' `accel` attribute.
- **Tab context menu** (right-click a tab): Close, Close Others, Close to the Right | Copy Full Path, Copy File Name, Copy Directory Path | Open Containing Folder, Open Terminal Here | Rename File…, Move to Trash…. The commands act on the clicked tab; those that need a file are greyed out for an untitled tab.
- **Editor context menu:** GtkTextView's own (Cut, Copy, Paste, Delete, Undo, Redo, Select All, Insert Emoji), then Convert Case to ›, Comment/Uncomment ›, Line Operations › (the same submenus as Edit) and Select and Find Next, Go to Matching Brace.
- **Dialogs** (AdwAlertDialog, square, boxed lists square too): Add Prefix/Suffix (Prefix, Suffix), Insert Numbers (Start, Step, Repeat each number, Base, Uppercase hex digits, Fill to the widest, Minimum width), Rename File (New name, with the name before its extension selected), the Move to Trash and Set as Default Editor confirmations and the latter's report. The first field has the focus, Tab moves on, Enter in a field applies, Escape cancels; the tool dialogs remember their values.
- **Keyboard Shortcuts** (Help): libadwaita's shortcuts dialog with a search field and one section per menu (and the palette's commands), each command with its keys from the registry and keys.toml, the Edit submenu and "in the editor" as the subtitle of editor-scoped keys.
- **Toasts** for the text tools' errors ("Invalid JSON: expected value at line 1, column 13"), Valid JSON and Well-formed XML, refusals (read-only, still loading), copied paths, renames, trashing, a created settings file, and a settings file with mistakes ("keys.toml not applied: line 1, column 33: …").

## As built in M2 — 2026-10-01

Headless screenshots: [m2-restored-conflict-tokyo-night](../tools/progress/public/m2-restored-conflict-tokyo-night-2026-10-01.png), [m2-lost-changes-catppuccin-latte](../tools/progress/public/m2-lost-changes-catppuccin-latte-2026-10-01.png), [m2-forget-drafts-gruvbox](../tools/progress/public/m2-forget-drafts-gruvbox-2026-10-01.png). It adds to the entries below.

- **Quitting never asks:** closing the window (SUPER+W), Quit (Ctrl+Alt+Q, Alt+F4) and logout keep every tab and its unsaved text; the window goes at once and the session is written behind it. The one question left is about documents in large-file mode with unsaved changes, which have no backups: "Save changes to …?" with Save All, Discard All and Cancel, listing only those. Closing a single dirty tab (Ctrl+W, the tab's ×) still asks Save, Discard or Cancel. Started with `--no-session`, Stet asks on quitting as before.
- **Restoring** brings back the tabs in their order, the one in front, every tab's dirty dot, the window's size and maximized state, the zoom, the recent files and the find bar's histories. Tabs other than the one in front show their title and dirty dot and load their text when first shown; while one loads it is read-only, as any loading file.
- **Banners** (the tab's stack, M3's order): a restored document whose file changed on disk after its unsaved changes were kept: "The file “plan.md” changed on disk since your unsaved changes." with **Keep Mine** (the next save writes over the file) and **Load Disk Version** (one step that Undo takes back); Compare comes with M7. Unsaved changes that could not be restored (their backup was lost, or a large-file document after a crash): "The unsaved changes to “notes.txt” from the last session could not be restored." with **Dismiss**; the file opens as it is on disk.
- **Toast:** "Recovered an unsaved document that was not in the last session" (or "Recovered N unsaved documents …") for backup files no session listed, which open as untitled tabs; and why the session is off, when another Stet holds it.
- **Forget Unsaved Drafts…** (File menu, above Quit, and the palette) asks first, in a destructive dialog: "Forget all unsaved drafts?" — untitled documents close, files with unsaved changes go back to what is on disk, and the backups are deleted; Forget Drafts or Cancel.

## As merged in Wave A — 2026-10-01

M3 and M4 merged; the entries below stand, and this one wins where they differ.

- **Layout, top to bottom:** the tab strip with the hamburger menu; the current tab, which is its banners (stacked, most urgent on top) above its editor; the find bar with its Find, Replace and Find in Files rows; the search results panel under a divider; the status bar (`Ln, Col, Sel`, language, line ending, encoding, `INS`/`OVR`). The banners belong to the tab; the find bar and the panel belong to the window and act on whichever tab is in front.
- **Keyboard focus:** the current tab's editor has it after a file opens (the command line, the Open dialog, a drop, a recent file, a search result), after a tab switch with the keyboard or a click on a tab, when a load ends, and when the find bar, the palette, a picker, a status-bar menu, the hamburger menu or a dialog closes, or a banner button is pressed. Two exceptions keep what the user chose: something that happens in the background (a load ending, a reload) leaves the focus where it is, in the find field for example; and Ctrl+Tab from the find field keeps the focus in the field, so the search follows the new tab. A click on a banner button leaves the focus in the editor; the buttons stay reachable with Tab.
- **Large files and search:** in large-file mode (50 MiB) the find bar counts and finds, but highlights neither every match nor the selected word, from the moment the file's size is known, before its text has loaded.

## As built in M3 — 2026-10-01

Built under the standing authorization of 2026-10-01; the headless screenshots [m3-banners-tokyo-night](../tools/progress/public/m3-banners-tokyo-night-2026-10-01.png), [m3-encoding-picker-catppuccin-latte](../tools/progress/public/m3-encoding-picker-catppuccin-latte-2026-10-01.png), [m3-unmappable-gruvbox](../tools/progress/public/m3-unmappable-gruvbox-2026-10-01.png), [m3-changed-on-disk-tokyo-night](../tools/progress/public/m3-changed-on-disk-tokyo-night-2026-10-01.png), [m3-long-lines-tokyo-night](../tools/progress/public/m3-long-lines-tokyo-night-2026-10-01.png) and [m3-formatted-tokyo-night](../tools/progress/public/m3-formatted-tokyo-night-2026-10-01.png) show it. It adds to the M1 entry below; where they differ, this one wins.

- **Banners** sit between the tab strip and the editor and belong to the tab. Several can show at once, stacked in a fixed order, most urgent on top: loading ("Loading “big.log”… 45%"), changed on disk (Reload, Keep Mine) or deleted (Save), read-only (Copy Command, and Edit Anyway when the file merely isn't writable), binary (Edit Anyway when it round-trips), placeholder conflict, lossy decode (Reinterpret…), mixed line endings (Normalize to CRLF, Keep), long lines (formatted, or broken for display), large file, highlighting off (Highlight Anyway). They use AdwBanner's CSS nodes (`banner > revealer > widget`), so libadwaita and the theme style them as banners, but they take any number of buttons; their text wraps and can be selected, so the `sudoedit` command can be copied by hand too.
- **Status bar:** `Ln, Col, Sel`, the language, the **line ending** and the **encoding**, then `INS`/`OVR`. The line ending (`LF`, `CRLF`, `CR`, or `Mixed`) opens a menu headed "Save line endings as": Windows (CR LF), Unix (LF), Macintosh (CR). The encoding shows the format the next save writes (`UTF-8`, `UTF-8 BOM`, `UTF-16 LE BOM`, `Windows-1252`, `Shift_JIS`) and opens the encoding picker.
- **Encoding picker:** a popover over the status bar with two toggles, Reinterpret As (read the file again) and Convert To (what the next save writes); untitled documents only convert. Below them a filter entry, then every encoding under group headings (Unicode, Western European, Central European, …), the current one selected. Up and Down move through the list from the filter, Enter or a click chooses. It is a popover of its own because a popover menu names submenu pages by their labels, and the same group names under both modes collided.
- **Menu:** Encoding sits between View and Language: Encode in UTF-8, UTF-8-BOM, UTF-16 BE BOM, UTF-16 LE BOM; Character Sets › one submenu per group; Convert to ANSI (Windows-1252), UTF-8, UTF-8-BOM, UTF-16 BE BOM, UTF-16 LE BOM. File has Reload from Disk; Edit has EOL Conversion › Windows (CR LF), Unix (LF), Macintosh (CR). The palette lists them all and "Encoding…", which opens the picker.
- **Dialogs** (AdwAlertDialog, square): the unmappable dialog ("“greek.txt” can't be converted to Windows-1252": how many characters have no equivalent, then a scrolling list of up to 20 rows "Line 1, column 1: Ω (U+03A9)" each with Go To, then Convert to UTF-8 (or Save as UTF-8) and Cancel); the long-line dialog ("“export.json” has very long lines": the longest line's length, then Open Formatted as JSON or XML, Open Read-Only, Cancel); questions before Reload from Disk or Reinterpret discards unsaved changes, and before a save writes formatted text, a lossy decode or mixed line endings; an error dialog when a save failed partway, naming the backup.
- **Tabs:** a spinner while the file loads; the dirty dot and `*` in the title when the file was deleted on disk.
- **Toasts:** a quiet reload ("“notes.txt” changed on disk and was reloaded"), Reload from Disk, Reinterpret, a file too large to open (with the limit), text that couldn't be formatted, a save refused for text broken for display, the copied `sudoedit` command.
- **Never** a tab switch, a focus change or a raised window because a file changed on disk.

## As built in M4 — 2026-10-01

Headless screenshots: [m4-replace-tokyo-night](../tools/progress/public/m4-replace-tokyo-night-2026-10-01.png), [m4-find-in-files-catppuccin-latte](../tools/progress/public/m4-find-in-files-catppuccin-latte-2026-10-01.png), [m4-error-and-smart-highlight-gruvbox](../tools/progress/public/m4-error-and-smart-highlight-gruvbox-2026-10-01.png). Where this differs from the M1 entry or the proposal below, this entry wins.

```
+------------------------------------------------------------------------------+
| editor                                                                       |
+------------------------------------------------------------------------------+
| [Find________] v  Aa W Sel [Regex v]  ^ v  3 of 12  message…          ⋮  x  |  find row (always)
| [Replace with_] v  [Replace] [Replace All] [Replace All in Open Documents]   |  Ctrl+H
| [Folder______] [o] [Filters] [x] Subfolders [ ] Hidden [x] .gitignore [Find in Files] |  Ctrl+Shift+F
+------------------------------------------------------------------------------+
| Search "TODO" (2 hits in 2 files of 4 searched) in 0.04 s    Stop Copy Clear x |  results panel
|  v Search "TODO" (2 hits in 2 files of 4 searched)                           |  (resizable divider)
|    v ~/proj/notes.md (1 hit)                                                 |
|        Line 3: - TODO: find in files                                         |
+------------------------------------------------------------------------------+
| status bar                                                                   |
+------------------------------------------------------------------------------+
```

- **Find bar:** one bar with Find, Replace and Find in Files as rows: Ctrl+F shows the find row, Ctrl+H adds the replace row, Ctrl+Shift+F the files row. It now sits inside the content, right below the editor and above the results panel (the M1 bar was a toolbar bottom bar); it is styled like the raised bars (`--headerbar-bg-color`, a 1 px top border). The find row: the field with a history drop-down, `Aa` (match case), `W` (whole word, greyed out in Regex mode), `Sel` (In selection), the mode drop-down (Normal, Extended, Regex), previous and next, the count ("3 of 12", "Searching…", "No matches", "Invalid pattern"), a message (Count and Replace All results, wrap-around notes, errors in the theme's error colour), `⋮` (Wrap around, Backward direction, `.` matches newline, Count, Find All in Current Document, Find All in Open Documents) and close. A pattern error shows on the field itself: error style, the message as tooltip and a wavy underline under the character it is about; the focus stays in the field. The replace field takes the warning style with an explanation as tooltip for a template that is easy to get wrong (grouping parentheses, a condition, a `)` that ends it), and the error style for one no text can hold. Enter and Shift+Enter act in every row (find, replace, start Find in Files), Up and Down walk the field's history, Escape closes.
- **Highlights:** all matches in the scheme's `search-match` style (GtkSourceView's, or our tag in the same colours for the queries our matcher runs), the current one in `stet:search-current`; smart highlighting of a selected word in `stet:smart-highlight`. Large documents (50 MiB) show neither all matches nor smart highlights.
- **Results panel:** below the find bar, in a vertical paned with a 1 px divider, hidden until a Find All or Find in Files, 220 px high at first and then as the user leaves it. A header line with the newest search's status (and the errors as tooltip) and Stop (while Find in Files runs), Copy, Clear and close; below it a tree of searches (newest first, older ones collapsed, at most 10), files (`~` for the home folder, hit counts) and lines (`Line n:` and the line, the matches in bold on the scheme's `search-match` background, recoloured live). Panel colours are the sidebar's (`--sidebar-bg-color`). Find in Files lists files in the order its parallel walk reports them (sorting them as they stream in stalled the main loop with thousands of files); Find All lists documents in tab order.
- **Menu:** Search holds Find…, Find in Files…, Find Next, Find Previous, Replace…, then Search Results Window, Next Search Result, Previous Search Result, then Go to Line…. The bar's buttons and the panel's commands are in the palette, not the menu.

## As built in M1 — 2026-10-01

Under the standing authorization of 2026-10-01 the M1 layout was built directly, without a separate mock gate; the headless screenshots [m1-editor-tokyo-night](../tools/progress/public/m1-editor-tokyo-night-2026-10-01.png), [m1-palette-catppuccin-latte](../tools/progress/public/m1-palette-catppuccin-latte-2026-10-01.png) and [m1-close-dialog-gruvbox](../tools/progress/public/m1-close-dialog-gruvbox-2026-10-01.png) stand in for it. Where the build differs from the proposal below, this entry wins.

- **Top bar:** AdwTabBar with the hamburger button as its end action, inside an AdwToolbarView top bar (RAISED_BORDER, so it shows `--headerbar-bg-color` with a 1 px border and no shadow). No title bar and no window controls on Hyprland (detected by `HYPRLAND_INSTANCE_SIGNATURE` or `XDG_CURRENT_DESKTOP`); elsewhere the tab strip is a window handle and ends with the window controls, so the app stays usable outside a tiling compositor. Tab titles are the file name, `● ` in front while dirty, and the full path as tooltip; untitled tabs are "Untitled N" with the lowest free N. Tabs are square.
- **Editor:** one GtkSourceView per tab, line numbers, current-line highlight, auto-indent, tab width 4 with tabs, smart Home. A banner above the editor appears only for read-only files (not UTF-8, or containing NUL).
- **Find bar:** below the editor, above the status bar, so the text never shifts: search entry, `Aa` (match case), previous, next, the match count ("3 of 12", "No matches") and close. While the bar has focus the current match is drawn in the theme's `stet:search-current` style (yellow), because a selection would use the faint unfocused colour; Escape (or close) turns it into the selection. Replace, whole word, modes and the other options are M4.
- **Command palette:** a popover at the top centre of the editor with a search entry and rows of label, menu path (dimmed) and every key (dimmed, right-aligned); `:line` or `:line:col` turns it into Go to Line, which Ctrl+G opens. Recent files are rows too.
- **Status bar:** `Ln, Col, Sel` (column counted with tabs expanded), then the language (a button that opens the language picker, a filterable popover list with Plain Text first), the line ending found when the file was opened (the buffer keeps line endings verbatim until M3), `UTF-8`, and `INS`/`OVR`. Monospace like the tab strip.
- **Dialogs:** AdwAlertDialog, square, for closing dirty tabs (Save, Discard, Cancel), Close All and quitting with unsaved changes (Save All, Discard All, Cancel; sessions arrive in M2). About is an AdwAboutDialog.
- **Toasts:** a save, a file that could not be opened (missing, too large, lines too long, not a regular file), a new file from the command line.
- **Menu:** as in the table below, limited to what works in M1: File (New, Open…, Open Recent ›, Save, Save As…, Save All, Close, Close All, Restore Closed Tab, Quit), Edit (Undo, Redo, Cut, Copy, Paste, Delete, Select All), Search (Find…, Find Next, Find Previous, Go to Line…), View (Word Wrap, Show Whitespace, Zoom In, Zoom Out, Reset Zoom, Full Screen, Next Tab, Previous Tab, Move Tab Forward, Move Tab Backward, Command Palette…), Language (Set Language…), Help (About Stet). Open Recent is hidden while the list is empty.

## Status and approval gate

This document was a **proposal** until M1; the entry above records what was built.

The user chose the minimal hybrid chrome on 2026-09-30. The exact layout is approved through a **mock and screenshot gate at the start of M1, before any UI work**: a mock of the tab strip, status bar, find bar, palette and banners, then screenshots of the first build in `tools/progress/`, both approved by the user and recorded as dated entries in [PLAN.md](PLAN.md). Any change the user asks for at that gate is recorded here as a dated entry above this section.

## Principles

The product-level UX principles are in [PRD.md](PRD.md#ux-principles). In short:

- **Omarchy-minimal hybrid:** tabs, editor, optional bottom panel, status bar; a hamburger menu and an F1 palette; **no toolbar and no classic menu bar**.
- **Keyboard-first, mouse-friendly:** every action has a menu entry and a palette entry generated from the `ActionId` registry; context menus on tabs and in the editor; clickable status-bar items.
- **Theme-driven:** every colour comes from the live Omarchy palette and every text face from the fontconfig `monospace` alias ([ADR-004](DECISIONS.md)).
- **Non-modal first:** bars, banners and toasts; dialogs only for decisions.

## Window layout

```
+------------------------------------------------------------------------------+
| [=] | * main.rs  x | notes.txt  x | Cargo.toml  x |                          |  1 hamburger + tab strip
+------------------------------------------------------------------------------+
| File changed on disk.                       [Reload] [Keep mine] [Compare]   |  2 banner (only when needed)
+-----+------------------------------------------------------------+-----------+
|   1 | fn main() {                                                | ::::::::: |
|   2 |     println!("hello");                                     | :::::     |  3 editor + gutter
|   3 | }                                                          |           |    (+ optional map)
|     |                                                            |           |
+-----+------------------------------------------------------------+-----------+
| Find [ \d+_______ ]  Replace [ ________ ]  Aa  W  .*  Ext  Sel   3 of 12  ^ v x |  4 find bar (Ctrl+F/H)
+------------------------------------------------------------------------------+
| Find in Files: "TODO" in ~/Projects  -  42 hits in 7 files     [Cancel] [x]  |  5 bottom results panel
|  v src/main.rs (2)                                                           |    (F7, resizable)
|      12: // TODO: handle errors                                              |
+------------------------------------------------------------------------------+
| Ln 2, Col 5, Sel 0 | 3 lines, 38 chars | LF | UTF-8 | Rust | Spaces: 4 | INS |  6 status bar
+------------------------------------------------------------------------------+

                +--------------------------------------------+
                | > sort li_                                 |  7 F1 palette (overlay,
                |   Sort Lines Lexicographically   (no key)  |    top-centre)
                |   Sort Lines by Length                     |
                |   Remove Duplicate Lines                   |
                +--------------------------------------------+
```

1. **Tab strip** (AdwTabBar). It is the window's top bar: there is no separate title bar and no window-control buttons, because Hyprland tiles windows and `SUPER+W` closes them. Each tab shows the file name, a dirty dot and a close button; untitled tabs are "new 1", "new 2" (first-line names are 1.0; built in M8, see its entry). Tabs reorder by drag; a double-click on a tab renames it: its file (1.1), or an untitled tab's name, without saving (1.2). The tab context menu offers close others / close to the right; save and save as (1.1); copy path, filename or directory; open containing folder; open a terminal here; rename (a file, or an untitled tab's name since 1.2); move to trash. The **hamburger button** sits at the left end of the strip (since 1.3; at the right end before).
2. **Banners** (AdwBanner) sit between the tab strip and the editor and belong to the current tab. They appear only when something needs attention: file changed on disk while dirty (Reload / Keep mine / Compare in 1.0), deleted on disk, read-only file (points to `sudoedit`), lossy decode (`lossy_roundtrip`), mixed line endings, large-file mode, opened formatted by the long-line path, and restore conflicts.
3. **Editor** (GtkSourceView) with line numbers, current-line highlight and optional whitespace and EOL glyphs. The document map (GtkSourceMap) is an off-by-default toggle, disabled in large-file mode. Split view (1.0) puts two editors side by side in a GtkPaned.
4. **Find bar** (Ctrl+F, Ctrl+H for replace). Proposed position: below the editor, so the text never shifts when it opens; the M1 mock decides. Controls: pattern, replacement, match case, whole word, mode (Normal / Extended / Regex), wrap, in selection, backward, match count, previous / next, close. Escape returns focus to the editor.
5. **Bottom results panel** (GtkPaned, hidden until used) holds Find in Files and Find All results grouped by file, with a cancel button while a search runs. F7 focuses it; F4 / Shift+F4 step through results.
6. **Status bar**: Ln/Col/Sel, length and lines, EOL, encoding, language, indentation, INS/OVR. EOL, encoding, language and indentation are clickable and open popovers (convert EOL; reinterpret or convert encoding; language picker; tab width and spaces).
7. **F1 command palette**: a filterable list of every action with its shortcut, top-centre over the editor. It also opens `config.toml` and `keys.toml`.

**Window title:** `file — dir — Stet`, with a dirty marker. Hyprland draws no title bar, but the title is used by window switchers and window rules.

**Toasts** (AdwToast) confirm things that need no decision: a clean file reloaded from disk, a save, "Set as default editor" done.

**Dialogs** are in-window and appear only when a decision is required: closing a dirty tab with Ctrl+W, a character the target encoding can't represent, the long-line choice (open formatted, read-only with display breaks, open anyway; S1 proposes dropping or changing "open anyway", which awaits the user's review in [PLAN.md](PLAN.md#name-repository-and-spike-changes-approved--2026-09-30)), "Set as default editor", and "Forget unsaved drafts".

## Hamburger menu

The menu has conventional submenus, so users find things where they expect them. Proposed top level for the MVP, to be confirmed at the M1 gate:

| Submenu | Contents (MVP) |
| --- | --- |
| File | New, Open, Recent, Reload, Save, Save As, Save All, Close, Close All, Restore Closed Tab, Quit |
| Edit | Undo/redo, clipboard, Line Operations, Convert Case, Comment, Indentation, Add Prefix/Suffix, Insert Numbers, EOL Conversion |
| Search | Find, Replace, Find in Files, Find All, Go to Line, Go to Matching Brace |
| View | Word Wrap, Show Whitespace/EOL, Document Map, Zoom, Full Screen |
| Encoding | Reinterpret as…, Convert to… |
| Language | Language picker (by category) |
| Tools | JSON (format, minify, validate), XML (format, validate) |
| Settings | Open config.toml, Open keys.toml, Set as Default Editor, Forget Unsaved Drafts |
| Help | Keyboard Shortcuts, About |

The menu, the palette and the accelerators are generated from the same `ActionId` registry, so an entry cannot exist without a working action. There are no placeholder items.

## Theme tokens

All colours come from the resolved Omarchy palette (`omarchy-theme-color --all`), mapped in one provider layer ([ADR-004](DECISIONS.md)). Widgets never reference palette keys directly. The UI mapping below is implemented in `domain/src/theme.rs`. Spike S3 confirmed it headless on 2026-09-30 for the surfaces it sampled ([S3-theme.md](spikes/S3-theme.md)); the **M1 gate** confirms the rest, and the syntax mapping is from the approved plan. One S3 finding: `AdwToolbarView` bars are flat by default and show `--window-bg-color`, so the tab strip and status bar need the raised style or explicit CSS to show `--headerbar-bg-color`.

### UI roles

libadwaita variables were checked against the installed libadwaita 1.9.3 library; GtkSourceView style names against the installed 5.20.0 library.

| Role | Applied through | Omarchy key |
| --- | --- | --- |
| Window background | `--window-bg-color` | `background` |
| Editor background | `--view-bg-color`; scheme style `text` (background) | `background` |
| Text | `--window-fg-color`, `--view-fg-color` and the other `*-fg-color` variables; scheme `text` (foreground) | `foreground` |
| Tab strip, status bar | `--headerbar-bg-color` | `dark_background` |
| Bottom results panel | `--sidebar-bg-color` | `dark_background` |
| Popovers, palette, dialogs | `--popover-bg-color`, `--dialog-bg-color` | `darker_background` |
| Cards, hover rows | `--card-bg-color` | `lighter_background` |
| Borders, separators | `--border-color`, `--headerbar-shade-color` | `lighter_background` |
| Accent: focus ring, active-tab indicator, default button | `--accent-bg-color`, `--accent-color` | `accent` |
| Text on accent | `--accent-fg-color` | `background` (contrast checked in S3) |
| Error / warning / success | `--error-*`, `--warning-*`, `--success-*` | `red` / `yellow` / `green` |
| Window corners | `--window-radius` | `0` |
| Dark or light variant | AdwStyleManager FORCE_DARK / FORCE_LIGHT | `mode` |
| Selection | scheme `selection` | `selection_background` bg, `selection_foreground` fg |
| Caret | scheme `cursor` | `cursor` (always `bright_foreground`) |
| Current line | scheme `current-line` | `lighter_background`, mixed toward `foreground` when it equals `background` |
| Line numbers / current number | scheme `line-numbers` / `current-line-number` | `muted` / `foreground` |
| Whitespace and EOL glyphs | scheme `draw-spaces` | `muted` |
| Right margin | scheme `right-margin` | `lighter_background` |
| Bracket match / mismatch | scheme `bracket-match` / `bracket-mismatch` | `yellow` bold / `red` |
| All search matches | scheme `search-match` | `yellow` mixed into `background` |
| Current search match | named custom style `stet:search-current` | `yellow` bg, `background` fg |
| Smart highlight | named custom style `stet:smart-highlight` | `accent` mixed into `background` |
| Mark styles 1–5 and style tokens (1.0) | GtkTextTags, recoloured on theme change | `red`, `green`, `blue`, `magenta`, `cyan`, each mixed into `background` (as built in M7: the Find Mark Style `red` and the tokens `cyan`, `orange`, `yellow`, `magenta`, `green`, each 40 % into `background`; see the M7 entry) |
| Bookmark (M7) | scheme `stet:bookmark` (foreground), a disc in the gutter | `accent` |
| Compare (M7) | scheme `stet:compare-added`, `-removed`, `-changed`, `-moved`, `-changed-text`, `-padding` (background) | `green`, `red`, `blue`, `magenta` 20 %, `blue` 45 %, `muted` 25 %, into `background` |

Named custom styles live in the generated scheme and are re-applied after every theme swap. The resolved palettes of catppuccin-latte, flexoki-light, white and tokyo-night were checked on 2026-09-30: their `dark_background` and `darker_background` are slightly darker than `background` in both light and dark themes, so the chrome roles above stay legible in light themes too. S3 measured the contrast of these roles for all 22 built-in themes (see its contrast table). Two weak spots: the current line in lupine (1.04:1), and text on accent below 4.5:1 in rose-pine, miasma and catppuccin-latte. S3 proposes a guard for each.

### Syntax roles

The generated GtkSourceView scheme follows Omarchy's `helix.toml.tpl`:

| Style | GtkSourceView style ids | Colour |
| --- | --- | --- |
| keyword, statement | `def:keyword`, `def:statement`, `def:preprocessor` | `magenta` |
| function | `def:function`, `def:builtin` | `blue` |
| type | `def:type` | `yellow` |
| number, constant | `def:number`, `def:floating-point`, `def:decimal`, `def:base-n-integer`, `def:boolean`, `def:constant`, `def:special-constant` | `orange` |
| string | `def:string`, `def:character` | `green` |
| operator, special | `def:operator`, `def:special-char` | `cyan` |
| comment | `def:comment`, `def:doc-comment`, `def:shebang` | `muted`, italic |
| error | `def:error` | `red` |

Proposed extensions, also following `helix.toml.tpl`: `def:heading` red, `def:link-text` and `def:link-destination` blue, `def:insertion` / `def:deletion` green / red, `def:warning` yellow. `helix.toml.tpl` colours constants yellow; the plan uses `orange` (which falls back to `yellow` when a theme has no orange).

### Font

The editor, tab strip, status bar and palette use the family that `fc-match monospace` resolves (JetBrainsMono Nerd Font on this machine), re-resolved when `~/.config/fontconfig/` changes. Default size 11 pt; zoom is per session. Non-editor labels may use the UI font if the M1 mock looks better that way.

Tab stops are set in Pango units from the font's exact advance and re-applied when the tab width, font or zoom changes, so tabs stay on the character grid when the advance is fractional ([ADR-015](DECISIONS.md#adr-015--exact-tab-stops-in-pango-units-2026-09-30)).

## Square chrome rule

- `--window-radius: 0`. Tabs, buttons, entries, popovers, the palette, banners and the find bar have square corners.
- No drop shadows drawn by the app; separators are 1 px borders from the theme.
- The compositor owns the outer window corners. Omarchy's default is `rounding = 0`; this machine overrides it to 8 in `~/.config/hypr/looknfeel.lua`. Either way the app's own chrome stays square.
- No transparency effects; Omarchy's default window opacity rule applies from outside (see [INTEGRATIONS.md](INTEGRATIONS.md) for the opt-out snippet).

## Keyboard map summary

The full keymap is [ADR-009](DECISIONS.md); USER_GUIDE lists it in M9. Keys that shape the layout: F1 palette, Ctrl+F / Ctrl+H find bar, Ctrl+Shift+F Find in Files, F7 (Ctrl+Alt+R) results panel, F4 / Shift+F4 (Ctrl+Alt+Down / Up) results navigation, Ctrl+Tab document switching (the MRU switcher since M8; Ctrl+PgDn / Ctrl+PgUp in strip order), Ctrl+P quick open, Alt+Left / Alt+Right back and forward, F11 full screen, Escape closes the innermost bar or popover.
