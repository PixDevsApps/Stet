# Stet release notes

Project: Stet, a fast, keyboard-first text and code editor for Omarchy

## Unreleased

- **Restored tabs follow your highlighting limits.** A tab restored from its backup when Stet starts now decides syntax highlighting with `highlight_max_bytes` and `highlight_max_lines` from `config.toml`, as an opened file always did; before, it used the built-in limits. The banner that offers Highlight Anyway now names the limits in effect instead of the built-in ones.
- **The command palette and the pickers find like quick open.** The command palette (F1) and the language and encoding pickers rank what you type with quick open's matcher: word starts and runs of letters first, case ignored. In the pickers, Enter now takes the best match rather than the first visible row (typing `rust` and Enter picks Rust, not reStructuredText).
- **Insert Numbers refuses numbers that don't fit in 64 bits**, with the Column Editor's message, instead of writing them; and the Column Editor now accepts a few sequences it refused before although every number fit.
- Find in Files now also searches the unsaved text of an open document whose path was opened through a symbolic link.
- The settings files Open Settings creates, the default-editor setting and the generated colour schemes are now written atomically and synced to disk.
- Under the hood: a large clean-up of Stet's code with no other change to what it does ([ADR-020](DECISIONS.md#adr-020--code-architecture-layers-bounded-contexts-and-lints-2026-11-13)). The packages no longer contain Stet's own test harness (the hidden `--self-test` option).

## 1.6.0 — 2026-11-12

- **Stet as a Flatpak**, for Linux systems other than Omarchy and Arch, from Stet's own Flatpak repository: `flatpak install --user https://github.com/PixDevsApps/Stet/releases/latest/download/io.github.pixdevsapps.Stet.flatpakref`, then `flatpak update` for updates. It runs on GNOME's runtime and needs no network. In the Flatpak, Check for Updates… and Open Terminal Here aren't in the menu, Set as Default Editor… says how to choose Stet in your system's settings, and Stet keeps its settings and session in `~/.var/app/io.github.pixdevsapps.Stet/`. On Omarchy and Arch, the package stays the way to install Stet.
- The Arch package also installs Stet's AppStream description (`/usr/share/metainfo/io.github.pixdevsapps.Stet.metainfo.xml`), which software centres read.

## 1.5.1 — 2026-11-12

- **A tab dropped after the last tab stays in that strip.** A tab dragged onto the + or onto the empty part of a tab strip after it now becomes that strip's last tab, also in the other view's strip in split view. In 1.5.0 such a tab went back to where it came from.
- Two of Stet's own tests no longer fail now and then (the Keep Mine step of a self-test script, and a timing test). Stet itself is not changed by them.

## 1.5.0 — 2026-11-12

- **The + follows the tabs.** The + that opens a new tab now sits right after the last tab, as in a web browser, instead of next to the menu. When there are more tabs than fit, it stays at the end of the tabs on screen. In split view each tab strip has its own +, which opens the tab in its own view.
- **Submenus open when you point at them.** In the menu (☰), resting the pointer on File, Edit, Search or any other submenu opens it beside the menu; there is no need to click. Each submenu stays inside the window, and a long one scrolls.
- **Check for Updates…**, at the bottom of the menu, asks GitHub which version is the latest and tells you whether yours is. When a newer one is out, it shows the command that installs it, with a button that copies it. Stet asks only when you choose the command, sends nothing about you, and never installs anything itself.
- **A one-line install.** From this release on, every release carries its package as `stet-x86_64.pkg.tar.zst`, so the README's install line always installs the latest release and needs no change for a new version. Run the same line again to update.
- The package now depends on `curl`, which Check for Updates uses (pacman needs it too, so it is installed already).

## 1.4.1 — 2026-11-07

- **No change to Stet itself.** Its headless tests no longer read the desktop's GTK settings. On GTK 4.24 some of them stop because of a GTK bug, which is waiting for a fix from GTK.

## 1.4.0 — 2026-11-07

- **A + button for a new tab.** Right of the menu (☰) at the left end of the tab strip, a + opens a new untitled tab, as File › New (Ctrl+N) does. In split view it opens the tab in the view whose strip it is on.

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
