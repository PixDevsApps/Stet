# Product Requirements — Stet 1.0

Project: Stet, a fast, keyboard-first text and code editor for Omarchy

Synced: 2026-09-30

The repository docs are the canonical project memory ([ADR-012](DECISIONS.md)). Dated entries describe the state at that date; newer entries take precedence. The approved plan behind this document is summarised in [PLAN.md](PLAN.md).

---

# Product summary

**Stet** is a native, keyboard-first, theme-driven text editor for Omarchy, written in Rust with GTK4, libadwaita and GtkSourceView 5. It offers the features people actually use and never loses work: unsaved and untitled tabs survive quitting and rebooting with no prompts.

> **Core promise:** open anything, edit it with the muscle memory you already have, close the window whenever you like, and find everything where you left it.

It deliberately does **not** aim for full feature parity: the feature set comes from what the research found people use ([RESEARCH.md](RESEARCH.md)), and the keymap is the classic editor keymap ([ADR-009](DECISIONS.md)). Stet's name was chosen on 2026-09-30 ([ADR-010](DECISIONS.md)): the proofreading mark *stet*, Latin for "let it stand", cancels a correction, which fits the core promise. During research and M0 the working name was "Notes".

# Problem

Omarchy (4.0.4 on the development machine) has no GUI text or code editor that feels native to it:

- The default editor is nvim in a terminal (`SUPER+SHIFT+N` runs `omarchy-launch-editor`). This machine has it set to `code`.
- Omawrite, Omarchy's own writer, handles Markdown only, one document at a time.
- Kate and GNOME Text Editor ignore the Omarchy theme. On Omarchy, GTK and Qt apps only get dark or light, never the palette.
- GUI editors used as `$EDITOR` break `git commit`, because `omarchy-launch-editor` starts them detached and passes no wait flag (Omarchy #13037).
- Picking a GUI default editor does not change the MIME default, so double-clicking a text file still opens nvim (Omarchy #7810).

Research across Hacker News and Stack Overflow found that what people miss most after switching editors is **never losing work**. After that come the search toolbox, column editing, the built-in text utilities and file compare. See [RESEARCH.md](RESEARCH.md).

# Users

**Primary: the keyboard-first power user who moved from Windows to Omarchy.** Has years of muscle memory for the classic editor keymap (Ctrl+D, Ctrl+Q, F3, Ctrl+Shift+F), keeps dozens of untitled scratch tabs open, and uses regex replace and "remove duplicate lines" weekly. Does not want a modal editor or an IDE.

**Secondary: the Linux developer or sysadmin.** Edits configs, logs, CSV and JSON dumps. Needs encodings and line endings kept exactly, big files that don't freeze the UI, `sudoedit` and `git commit` integration, and Find in Files across a project. Often lives in the terminal but wants a fast GUI editor for these jobs.

**Tertiary: Arch/Wayland users outside Omarchy.** Omarchy integration enhances the app but is never required. Outside Omarchy it follows the portal's dark/light preference with built-in palettes.

# Positioning

| Alternative | What it is | Why it doesn't fill the gap |
| --- | --- | --- |
| **Omawrite** 0.5.0 | Omarchy's Qt Quick Markdown writer (`SUPER+SHIFT+W`); reads the live `colors.toml` | One Markdown document at a time. No tabs for plain text or code, no regex toolbox, no encodings. Stet is its sibling, not its replacement. |
| **Kate** 26.08 | Mature KDE editor with block selection, multi-cursor and stashed unsaved files | Large KF6 dependency tree; ignores the Omarchy palette; users report it can't be made Omarchy's default editor (discussion #13697). |
| **GNOME Text Editor** 51 (Arch 50.1) | GTK4/libadwaita/GtkSourceView with excellent drafts | No Find in Files, column mode or multi-cursor; only Adwaita dark/light on Omarchy. |
| **nvim** | Omarchy's default editor, in a terminal | Modal and terminal-only; the wrong tool for the primary users above. |

**One line:** the features power users actually use (tabs that never lose work, regex and Find in Files, column editing, encoding and line-ending control, a safe large-file mode) in a keyboard-first Omarchy app that follows the live theme. A sibling to Omawrite and Omacalc, written in Rust.

**We compete on** being native to Omarchy (live theme and font, Hyprland and MIME integration, `--wait`), on reliability, and on focus. **We do not compete on** feature parity.

# Goals

1. **Never lose work.** Session restore with backups of dirty and untitled tabs every 7 s; quitting shows no prompts; external changes never steal focus.
2. **The everyday toolbox.** Regex find/replace with Normal, Extended and Regex modes, Find All, Find in Files, line tools, JSON/XML formatting, and (in 1.0) column mode, marking, split view and compare.
3. **Files done right.** Detect, keep and convert encodings and line endings; safe save that keeps symlinks, mode and owner; binary, NUL and huge files handled without data loss or freezes.
4. **Native to Omarchy.** Live theme and font, square chrome, a desktop entry with correct MIME types, an opt-in "Set as default editor" command, and `--wait` for `GIT_EDITOR` and `SUDO_EDITOR`.
5. **Keyboard-first, mouse-friendly.** The classic editor keymap and the F1 palette, plus a hamburger menu and context menus so every action is also reachable with the mouse.

# Success metrics

## Performance targets

From the plan's verification section; measured by `tools/bench.py` and the M0 spikes. Changing a target needs an ADR.

| Measure | Target |
| --- | --- |
| Cold start | ≤ 400 ms |
| Second launch forwarding to the running instance | ≤ 100 ms |
| 50-tab session restore (interactive) | ≤ 500 ms |
| Keystroke to paint on a 10 MB file | < 16 ms |
| Open a 100 MB / 1M-line file, first screen | ≤ 1.5 s, RAM ≤ 3× file size |
| 10 MB single-line JSON through the long-line path | No freeze over 2 s |
| Regex replace-all over 1M lines | ≤ 5 s, one undo step |
| Find in Files over `~/Projects` | First results < 300 ms; cancels instantly |
| Column insert at column 0 across 10k lines (1.0) | < 200 ms, one undo step |
| Undo or redo of a 10k-line column edit (1.0) | < 200 ms each |
| Compare two 10k-line files (1.0) | < 1 s |
| Format a 20 MB JSON file | UI never blocks |

## Reliability and integration

- Open 40 dirty and untitled tabs, then `kill -9`: everything comes back, with ≤ 7 s of edits lost.
- `SUPER+W`, `systemctl --user stop` on the app scope, and a reboot all restore without prompts.
- Every encoding fixture either round-trips byte-for-byte or is flagged lossy when opened.
- Saving through a symlink keeps the link.
- `omarchy theme set` recolours the app live; `omarchy-font-set` changes its font live.
- `GIT_EDITOR="stet --wait" git commit` works whether or not Stet is already running.
- After "Set as default editor", `SUPER+SHIFT+N` and double-clicking a file in Nautilus open Stet.
- Every MVP action works from its shortcut, the menu and the palette.
- The full manual Omarchy acceptance list in [TESTING.md](TESTING.md) passes or is recorded in release status.

# Feature tiers

MVP means version 0.3, reached through M0–M5. Lean 1.0 is reached through M6–M9. 1.x means after 1.0. Milestones are in [PLAN.md](PLAN.md).

## MVP (0.3)

**Documents and sessions**
- Tabs: new, open, save, save as, save all, close, close others/right, restore closed tab (Ctrl+Shift+T), reorder, dirty dot.
- Recent files; drag-and-drop from Nautilus.
- Session restore with backups of dirty and untitled tabs every 7 s. Quitting shows no prompts; closing a dirty tab with Ctrl+W asks.
- Single instance, and CLI `file[:line[:col]]`, `-n`, `-c`, `-l`, `--read-only`, `--no-session`, `--wait` (usable with `GIT_EDITOR` and `SUDO_EDITOR`).

**Files**
- Encodings: detect, keep, reinterpret, convert. Covers UTF-8, UTF-8 with BOM, UTF-16LE/BE, Windows-1252 and the rest of the `encoding_rs` set.
- Line endings: detect, keep, convert, with a "Mixed" banner.
- Safe save that keeps symlinks, mode and owner.
- A banner for read-only files, pointing to `sudoedit`.
- Binary files and NUL characters handled safely.
- Large-file mode and a long-line guard.
- External-change reload that never steals focus.

**Editing and view**
- Syntax highlighting (180 GtkSourceView languages) with detection by extension, filename and shebang, plus a language picker.
- Show whitespace and EOL, word wrap, line numbers, current-line highlight, zoom, full screen.
- Document map as an off-by-default toggle, disabled in large-file mode.
- Indentation: tab width and spaces per language, auto-detected, with a clickable status-bar item.

**Search**
- Find/replace bar with Normal, Extended (`\n \t \xHH`) and Regex (PCRE2) modes.
- Options: case, whole word, wrap, in selection, backward. Also count, replace all, highlight all, smart highlight, F3/Shift+F3, Ctrl+F3, go to line, brace jump and select.
- Find All in the current document, Find All in all open documents, and Replace All in all open documents.
- Find in Files with filters (including `!` excludes), hidden and .gitignore toggles, a results panel (F7, F4/Shift+F4) and cancel.

**Text tools**
- Line operations: duplicate, cut/copy/delete line, move up/down, join, sort (lexical ± case, numeric, length, reverse), remove duplicates, remove empty lines, trim, TAB↔space.
- Add prefix or suffix to lines, and insert incrementing numbers. These cover the top column-mode use cases before rectangular selection exists.
- Case conversion (UPPER, lower, Proper, Sentence, iNVERT); comment toggle, comment, uncomment and block comment.
- JSON format, minify and validate; XML format and validate.

**Chrome**
- Status bar: Ln/Col/Sel, length and lines, EOL, encoding, language, indentation, INS/OVR.
- Window title `file — dir — Stet` with a dirty marker.
- Tab context menu: copy path, filename or directory; open folder; open a terminal here; rename; trash.
- Editor context menu: case, comment, line operations, find selection.
- F1 palette covering every action.
- Config and keymap files, `config.toml` and `keys.toml`, with hot reload. The palette opens them.

**Omarchy**
- Live theme and font, with dark or light from `mode`.
- `.desktop` file and MIME types.
- A "Set as default editor" palette command that is opt-in and asks for confirmation.

## Lean 1.0 (beyond the MVP)

- **Column mode.** Alt+drag and Alt+Shift+arrows, column typing, delete, copy and paste, virtual space. Plus the Column Editor (Alt+C).
- **Marking.** Bookmarks (Ctrl+F2, F2, Shift+F2); Mark (Ctrl+M, 5 styles, bookmark the line) with bookmarked-line operations; style tokens.
- **Views.** Split view, clone and move to the other view (F8); compare two files (and against the clipboard).
- **Navigation.** Quick open (Ctrl+P), MRU switcher (Ctrl+Tab), back and forward, pinned tabs, first line as the untitled tab's name.
- **Replace in Files.**

## 1.x (after 1.0)

- Code folding, macros, multi-caret, Run (F5), tail -f mode.
- Word completion, auto-close pairs.
- A save-as-administrator helper using pkexec.
- A Preferences GUI and keymap editor, and a VS Code-style preset.
- The full character-sets menu and OEM code pages.
- Document list and folder workspace, print, `recent.json`, and an omarchy-shell plugin.

## Later

Function list and structural folding (tree-sitter), importing user-defined language files in other formats (user `.lang` files work from day one), spell check, hex view, JSON tree view.

## Never

- Plugin ABI or scripting (a security surface).
- A style configurator (the theme drives colours).
- An FTP plugin, an auto-updater, always-on-top (not possible on Wayland), clipboard history.
- Markdown preview (Omawrite covers it), i18n for 1.0, named session files.

# Non-goals

- Full feature parity, or classic chrome (menu bar, toolbar, many modal dialogs).
- An IDE: no language servers, debugger, build system or project model in 1.0.
- Plugins, scripting or remote editing (use sshfs or gvfs mounts).
- Colour customisation beyond the Omarchy theme and an optional per-theme override file.
- Windows or macOS builds; X11-only features.
- Accounts, telemetry, cloud sync or an auto-updater. Updates come through pacman.
- Changing user configuration automatically. The package never edits `~/.config`; "Set as default editor" is opt-in and asks first.

If an attractive idea comes up, add it to the 1.x or Later list in [PLAN.md](PLAN.md) instead of expanding 1.0.

# UX principles

- **Omarchy-minimal hybrid chrome.** A slim tab strip, the editor, an optional bottom results panel and a status bar. A hamburger menu with conventional submenus (File, Edit, Search, View, …), and an F1 command palette listing every action with its shortcut. **No toolbar** and no classic menu bar. The layout is in [DESIGN.md](DESIGN.md).
- **Keyboard-first, mouse-friendly.** The classic editor keymap is the default, so years of muscle memory work out of the box. Every action is also reachable from the menu and the palette; tabs and the editor have context menus; status-bar items are clickable; files can be dragged in.
- **Theme-driven.** Colours and the monospace font come from the live Omarchy theme and fontconfig. Square corners, no drop shadows, no hard-coded palette except the fallbacks outside Omarchy.
- **Never lose work, never nag.** Quitting never prompts; the session comes back. Only closing a dirty tab with Ctrl+W asks.
- **Never steal focus.** External changes reload clean files in place with a toast, and show a banner for dirty ones. The app never switches tabs on its own.
- **Non-modal first.** Find is a bar, problems are banners, confirmations are toasts. Dialogs appear only when a decision is required (for example a character the target encoding can't represent).
- **Honest.** No menu item that does nothing. Banners say exactly what happened (lossy decode, read-only file, mixed line endings, formatted-on-open). Performance claims come from measurements.
- **Safe with big files.** The UI never freezes; large-file mode and the long-line path degrade features instead.
- **Works outside Omarchy.** Missing Omarchy files mean fallback palettes, not errors.

# Defaults chosen

These can change during M0 without re-planning:

- Clean files that change on disk reload silently, with a toast.
- Large-file mode starts at 50 MB and the cap is 256 MB.
- Syntax highlighting switches off at its own, lower size threshold in bytes and lines ([ADR-014](DECISIONS.md#adr-014--syntax-highlighting-size-policy-2026-09-30)). An M3 measurement sets the default; the working assumption is roughly 10–20 MB.
- CJK fixtures are included only if the user wants them.
- Local PKGBUILD only; the AUR comes later.
