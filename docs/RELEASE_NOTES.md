# Stet release notes

Project: Stet, a fast, keyboard-first text and code editor for Omarchy

## 1.3.4 — 2026-10-02

- **Stet's repository is public**, at [github.com/PixDevsApps/Stet](https://github.com/PixDevsApps/Stet). Stet itself works as in 1.3.3.

## 1.3.3 — 2026-10-02

- **New wording.** `stet --help`, the package description, the description your app launcher shows for Stet, the comment at the top of the `keys.toml` that Settings › Open Keyboard Shortcuts creates, and the README's introduction are reworded. `stet --help` and the package description call Stet "a fast, keyboard-first text and code editor for Omarchy", as About does since 1.3.2.

## 1.3.2 — 2026-10-02

- **A narrower menu that stays in the window.** Each page of the menu is as wide as its own items: the main menu is a short column of File, Edit, Search…, and a submenu widens it only as far as its own items need. Until now every page was as wide as the widest submenu. The menu opens from the ☰ button into the window (since 1.3.1), and in a short window a long submenu scrolls instead of reaching past the window's bottom edge.
- **About Stet says more.** Help › About Stet describes what Stet is and does, links to its website and to where to report an issue, and shows the copyright.
- **A new icon:** the note on its own, without the dark tile around it.

## 1.3.1 — 2026-10-01

- **The menu opens into the window.** In 1.3.0 the menu was centred on the ☰ button, so half of it hung outside the window when the window didn't start at the left edge of the screen. It now opens from the button's left edge.

## 1.3.0 — 2026-10-01

- **The menu is on the left.** The hamburger menu (☰) now sits at the left end of the tab strip, before the tabs. In split view it is at the start of the first strip on screen.

## 1.2.0 — 2026-10-01

- **Name your tabs without saving.** Double-click an untitled tab, or use Rename… in its right-click menu, and give it a name: nothing is saved, the tab keeps the name as you type, Save As… proposes it, and it is still there after a restart. An empty name goes back to the tab's first line. Rename File… is now Rename…; on a saved file it still renames the file.

## 1.1.0 — 2026-10-01

Two additions asked for after 1.0:

- **Double-click a tab to rename its file.** It opens the same dialog as Rename File…, with the name before its extension selected. On an untitled tab, a double-click opens Save As…, which gives the document its name.
- **Save and Save As… in the tab menu.** Right-click a tab to save it, or save it under another name, without switching to it first.

## 1.0.0 — 2026-10-01

The first release: a native, keyboard-first text editor for Omarchy (Arch + Hyprland) with the features people use most. Written in Rust on GTK 4, libadwaita and GtkSourceView 5. The [user guide](USER_GUIDE.md) has the details; the [release status](RELEASE_STATUS.md) has what was checked and what is still the user's to check.

**Never lose work.** Unsaved and untitled documents are backed up every 7 seconds and come back after closing the window, logging out or a crash, without prompts. `stet --wait` works as `GIT_EDITOR` and `SUDO_EDITOR`, whether Stet is running or not.

**Omarchy, live.** The editor and its chrome follow the Omarchy theme and monospace font as they change, light themes included. Single instance with `stet file:line:col`; floating file dialogs; SUPER+C/V/X; an optional "Set as Default Editor" for SUPER+SHIFT+N and the file manager.

**Files done right.** Encodings are detected and kept (UTF-8 with or without BOM, UTF-16, Windows code pages, Shift_JIS and the rest of `encoding_rs`), with reinterpret and convert; line endings are kept or converted; saving is atomic and keeps symlinks, permissions and owner. Files up to 256 MiB load in pieces while the window keeps working (large-file mode from 50 MiB), very long lines are handled safely, binary and read-only files are guarded, and changes on disk reload without taking focus.

**Search.** A find bar with Normal, Extended and PCRE2 Regex modes and Boost-style replacement templates; Find All, Replace All in open documents, Find in Files and Replace in Files (keeping every file's encoding, BOM and line endings) with a results panel; smart highlighting; Mark with five styles and bookmarked-line operations; style tokens.

**Text tools.** Line operations, twelve sorts, case conversion, comments, blank and whitespace tools, prefix/suffix and number insertion, JSON and XML formatting and validation, brace jumps; each one undo step, large inputs on background threads.

**Column mode.** Rectangular selection with Alt+Shift+arrows or Alt+drag, typing and deleting on every row with virtual space, a rectangular clipboard, and the Column Editor (Alt+C).

**Navigation and views.** Quick open (Ctrl+P), the Ctrl+Tab recent-document switcher, back and forward (Alt+Left/Right and the mouse's side buttons), pinned tabs, bookmarks, split view with move and clone, and side-by-side compare (files, the clipboard, the saved version) with next and previous difference.

**Keys and settings.** A classic editor keymap with alternatives for every F-key (for keyboards whose F-row sends media keys), arranged around fcitx5 and the Norwegian layout; `keys.toml` and `config.toml`, reloaded as they change; a command palette (Ctrl+Shift+P or F1) with every command.

**Accessibility.** Every control has an accessible name for screen readers, including menu items (a GTK 4.22 gap Stet works around), and the keyboard focus returns to the text after every dialog, popover and menu.

### Known limitations

See [RELEASE_STATUS.md](RELEASE_STATUS.md#known-limitations-no-p1): GTK keeps one core busy for about 40 seconds after opening a file with a million lines (typing stays responsive), and undoing a very large operation freezes the window briefly.

### Earlier milestones

- 0.3.0 (2026-10-01): the MVP, milestones M1–M5.
- 0.2.0 (2026-10-01): search, milestones M1, M3 and M4.
