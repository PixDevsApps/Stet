# Research

Project: Stet, a fast, keyboard-first text and code editor for Omarchy

Synced: 2026-09-30

The repository docs are the canonical project memory ([ADR-012](DECISIONS.md)). Dated entries describe the state at that date; newer entries take precedence.

---

## About this digest

On 2026-09-30 a research workflow ran five parallel investigations: the features users of a fast, keyboard-first text editor use and expect, Omarchy 4.0.4 internals, Rust GUI toolkits, core crates, and prior art. An architect turned them into a draft plan, and two adversarial reviews (technical feasibility and scope/user value) critiqued it. The user approved the corrected plan the same day ([PLAN.md](PLAN.md)). The research ran under the working name "Notes"; the product has been named Stet since 2026-09-30 ([ADR-010](DECISIONS.md)).

This page condenses those reports. It keeps versions, evidence and sources, and marks where the approved plan **overrode** a research recommendation ("Plan decision"). Items the researchers could not verify are flagged and collected in [section 7](#7-open-and-unverified-items). Spike measurements are summarized in [M0 spike results](#m0-spike-results) and detailed in `docs/spikes/`.

Local inspection was read-only. Machine baseline on 2026-09-30:

| Component | Version |
| --- | --- |
| Omarchy (`omarchy`, `omarchy-settings`) | 4.0.4-1 (`/usr/share/omarchy/version` still says `4.0.0.alpha`) |
| gtk4 / libadwaita / gtksourceview5 | 4.22.4 / 1.9.3 / 5.20.0 |
| glib2 / pcre2 | 2.88.3 / 10.48 |
| fcitx5 | 5.1.22 |
| shared-mime-info | 2.5.1 |
| rustc / cargo | 1.98.1 |
| Current theme | `dark-music2` (Aether-generated, `mode = "dark"`) |
| Default editor state file | `code` |

Evidence notation used below:

- **SO Nk**: view count of the top Stack Overflow question the research found on the topic (StackExchange API).
- **HN id**: a Hacker News comment id (via hn.algolia.com). **MUO**: a MakeUseOf article (May 2026).
- **TC-n / SC-x**: finding n of the technical critique / finding x of the scope critique ([section 6](#6-critique-corrections)).

---

## 1. Feature usage

### Baseline

- Stack Overflow 2025 survey: 75.9% of respondents use VS Code, 24.3% Vim and 10.5% Sublime Text.
- Trust matters: an editor's own update mechanism is an attack surface. Updates through pacman avoid that class of risk.

### Ranked features

Ranked by strength of evidence. "Research" is the tier the report proposed; "Plan" is the approved tier ([PRD.md](PRD.md#feature-tiers)).

| # | Feature | Key evidence | Research | Plan |
| --- | --- | --- | --- | --- |
| 1 | Session restore + periodic backup of dirty/untitled tabs (7 s default) | HN 37960729 "Only one feature I miss…", 42793075, 39854782, 30023422 ("killer feature"), 43673722 ("40+ unsaved tabs"); SO 179k/31k on *disabling* it | MVP | MVP |
| 2 | Tabs, Ctrl+Tab, restore closed tab, pinned tabs, first-line names | HN 25628471 ("100s of tabs"); requests for pinning | MVP (pin, first-line 1.0) | MVP (pin, first-line 1.0) |
| 3 | Find/Replace: Normal, Extended, Regex | SO 676k "Find CRLF", 553k "replace new lines with comma", 500k, 408k, 163k; HN 11499299 | MVP | MVP |
| 4 | JSON/XML pretty-print, minify, validate | SO **2.26M** "format XML" (#1 question), **1.92M** "reformat JSON", 1.51M, 438k | MVP | MVP |
| 5 | Line operations (duplicate, move, sort, dedupe, remove empty, trim, TAB↔space, join) | SO **1.33M** "add to every line", 1.02M tabs→spaces, 972k dedupe, 740k empty lines | MVP | MVP |
| 6 | Column/block mode and multi-cursor | SO 1.33M, 300k "multi editing", 135k, 33k; HN 25149053, 27233843, 24565995 | MVP | Prefix/suffix + numbering MVP; column 1.0 (M6); multi-caret 1.x |
| 7 | Compare | SO **1.28M**; HN 3755659 | 1.0 | 1.0 |
| 8 | EOL conversion and display | SO 676k, 225k, 219k, 162k, 108k | MVP | MVP |
| 9 | Show whitespace/EOL/non-printing | SO 470k; 279k "removing NUL characters" | MVP | MVP |
| 10 | Syntax highlighting | universal; SO 64k "auto-set language" | MVP | MVP (180 GtkSourceView languages) |
| 11 | Encodings: reinterpret vs convert, character sets | SO 350k "ANSI to UTF-8", 100k, 85k | MVP subset | MVP (encoding_rs set); character-sets menu, OEM 1.x |
| 12 | Hex view | SO 387k + 317k | Later | Later |
| 13 | Case conversion | SO 321k | MVP | MVP |
| 14 | Themes / dark mode | **one of the most-requested features** | MVP (follow Omarchy); style configurator Never | Same |
| 15 | Run (F5), command execution | SO 258k, 95k | 1.0 | 1.x |
| 16 | Folder as workspace | SO 241k | 1.0 | 1.x |
| 17 | Auto-completion | SO 224k enable **vs 173k disable** | 1.0 (manual trigger) | 1.x |
| 18 | Comment toggle | SO 205k, 36k | MVP | MVP |
| 19 | Macros | SO 163k, 150k | 1.0 | 1.x |
| 20 | Column Editor (Alt+C) | SO 154k, 122k | 1.0 | 1.0 (M6) |
| 21 | Document switcher / list | SO 142k | MVP switcher | MRU switcher 1.0; list 1.x |
| 22 | Find in Files + results panel | SO 183k, 106k | MVP; replace 1.0 | Same |
| 23 | Code folding | SO 113k | 1.0 | 1.x |
| 24 | User-defined languages | SO 105k; HN 25149598 | Later | Later (user `.lang` files day one) |
| 25 | Mark (Ctrl+M) and bookmarks, bookmarked-line ops | HN 43759957 | MVP | 1.0 (M7) |
| 26 | Smart highlight; style tokens | universal default | MVP; tokens 1.0 | Same |
| 27 | Brace matching | SO 54k | MVP | MVP |
| 28 | Split view / clone | SO 54k, 24k | 1.0 | 1.0 |
| 29 | Change detection; tail -f | **a much-requested fix: never switch tabs when a file changes on disk** | MVP; tail 1.0 | MVP; tail 1.x |
| 30 | Large-file handling | SO 646k "read a 2 GB file", 100k | MVP | MVP |
| 31–34 | Wrap and zoom; incremental search; recent files; drag-and-drop | ubiquitous; SO 84k tab to new window | MVP | MVP |
| 35 | Command line and single instance | SO 21k, 12k | MVP | MVP (+ `--wait`) |
| 36 | Status bar | a much-requested word count | MVP | MVP |
| 37 | Shortcut mapper | SO 20k | TOML MVP; GUI 1.0 | `keys.toml` MVP; GUI 1.x |
| 38 | Base64/URL/hash tools | SO 41k, 16k | 1.0 | Not planned (SC-H1) |
| 39–40 | Spell check; function list | low; SO 22k | Later | Later |
| 41 | Document map | SO ~310 views (weak) | Later | MVP as off-by-default toggle |
| 42 | Auto-close pairs | moderate | 1.0, off | 1.x |
| 43 | Full screen / distraction-free | low | 1.0 | MVP (full screen) |
| 44 | Print | SO ~3k | Later | 1.x |
| 45–49 | Always on top; style configurator; FTP plugin; plugin ABI; clipboard history | — | Never | Never |
| 50–51, 53 | Hide lines; change-history margin; insert date/time | low | Later / 1.0 | Not planned |
| 52 | Save with elevated rights | a Linux-specific need | 1.0 (pkexec) | MVP banner + `sudoedit`; pkexec helper 1.x |
| 54 | Localization, auto-updater, shell extension | — | Never | Never (i18n not in 1.0) |
| + | Command palette, fuzzy open, back/forward | HN 27421476, 32026378, 28257308 (missing Ctrl+P); SO 20k | 1.0 | Palette MVP; quick open, back/forward 1.0 |

### What users miss most after switching editors

1. **Unsaved tabs that survive quit and reboot with no prompts** (HN 37960729, 42793075, 39854782, 30023422, 43673722, 21464961). Kate, gedit and VS Code have partial versions; users say none is as effortless.
2. **The search toolbox:** Extended mode, regex, Find in Files with a results tree, and Mark → bookmark lines → operate on (un)bookmarked lines (HN 43759957).
3. **Column mode, multi-editing and the Column Editor** (SO 1.33M, 300k, 154k, 135k).
4. **One-click text utilities:** dedupe, remove empty lines, tabs↔spaces, case, EOL conversion, JSON/XML pretty-print (SO 2.26M, 1.92M, 1.02M, 972k, 740k, 676k).
5. **Compare two files** (SO 1.28M; HN 3755659).

Runners-up: speed, user-defined languages, encoding control, hidden characters, dark mode. Several HN comments say no Linux editor covers all of this (HN 45156503, 42021678); Wine breaks on fonts (HN 42021558).

### Default shortcuts

The classic editor keymap's defaults (read 2026-09-30):

- **File:** New Ctrl+N, Open Ctrl+O, Reload Ctrl+R, Save Ctrl+S, Save As Ctrl+Alt+S, Save All Ctrl+Shift+S, Close Ctrl+W, Close All Ctrl+Shift+W, Restore Closed Ctrl+Shift+T, Print Ctrl+P, Exit Alt+F4.
- **Edit:** Duplicate Ctrl+D; cut/delete/copy line Ctrl+L / Ctrl+Shift+L / Ctrl+Shift+X; transpose Ctrl+T; move line Ctrl+Shift+Up/Down; blank line above/below Ctrl+Alt+Enter / Ctrl+Alt+Shift+Enter; join Ctrl+J; split Ctrl+I; UPPER Ctrl+Shift+U, lower Ctrl+U, Proper Alt+U, Sentence Ctrl+Alt+U (blend variants add Shift); toggle comment Ctrl+Q, comment Ctrl+K, uncomment Ctrl+Shift+K, block comment Ctrl+Shift+Q; Column Editor Alt+C; column select Alt+drag or Alt+Shift+arrows; extra caret Ctrl+click; word completion Ctrl+Enter, function completion Ctrl+Space; word-part left/right Ctrl+/ and Ctrl+\. Ctrl+C/X with no selection act on the line.
- **Search:** Find Ctrl+F, Replace Ctrl+H, Find in Files Ctrl+Shift+F, Mark Ctrl+M; F3 / Shift+F3; Ctrl+F3 / Ctrl+Shift+F3 (select word and find); volatile find Ctrl+Alt+F3 / Ctrl+Alt+Shift+F3; incremental Ctrl+Alt+I; Go to line Ctrl+G; brace Ctrl+B / select Ctrl+Alt+B; results F7, F4 / Shift+F4; bookmarks Ctrl+F2 / F2 / Shift+F2; style tokens Ctrl+1..5.
- **View:** F11 full screen, F12 Post-it; zoom Ctrl+NumPad+/−, reset Ctrl+NumPad/; folding Alt+0, Alt+1..8; other view F8; Ctrl+Tab / Ctrl+Shift+Tab; Ctrl+PgUp/PgDn; no default keys for wrap, show-all or document map.
- **Macro/Run/Help:** record Ctrl+Shift+R, play Ctrl+Shift+P, Run F5, About F1.

### Keys on Omarchy

- **Never reach the app:** Ctrl+Space (fcitx5 trigger; Omarchy #4842); Ctrl+Alt+F1–F12 (VT switching under Hyprland); Ctrl+Shift+U and Ctrl+Alt+Shift+U (fcitx5 Unicode addon; the report called this likely, TC-6 confirmed both in `/usr/lib/fcitx5/libunicode.so`).
- **Safe:** the classic editor keymap has no Super defaults, and Omarchy puts almost everything on SUPER. Omarchy's non-SUPER globals are Alt+Tab (±Shift), Ctrl+Alt+Tab (±Shift), **Ctrl+Alt+Delete (closes all windows)**, Print, Alt+Print and F9 (only with voxtype). Alt+drag is free (Omarchy's mouse binds use SUPER). `SUPER+C/V/X` send Ctrl+C/V/X to non-terminal windows.
- **Convention clashes:** Ctrl+Q quits most GTK/Qt apps; Ctrl+D, Ctrl+P, Ctrl+Shift+P, Ctrl+/ and Ctrl+L mean other things in VS Code (the user's current default editor); many laptops lack a numpad, so Ctrl+= / Ctrl+− zoom aliases were suggested.
- **Plan decision:** the classic editor keymap, these defaults with remaps ([ADR-009](DECISIONS.md)); a VS Code-style preset is 1.x.

### Performance baselines

- MUO 500 MB log test (May 2026):

| Editor | Open | RAM | Scrolling | Search near end |
| --- | --- | --- | --- | --- |
| Geany | 2.10 s | 1.1 GB | smooth | 8.2 s |
| Sublime Text | 11.14 s | 758 MB | smooth after load | 16.8 s |
| Notepad3 | 4.37 s | 1.2 GB | sluggish | 19 s |
| Kate | — | — | mildly sluggish | 38 s |

- The report proposed cold start ≤ 300 ms, forwarding ≤ 50 ms, 100 MB first screen ≤ 1 s and RAM ≤ 2×. **Plan decision:** the targets were relaxed for GTK to cold start ≤ 400 ms, forwarding ≤ 100 ms, 100 MB first screen ≤ 1.5 s with RAM ≤ 3× ([TESTING.md](TESTING.md#benchmarks-and-performance-targets)).

---

## 2. Omarchy 4.0.4 internals

### Key findings

1. **Omarchy's own GUI apps read `colors.toml` directly.** omawrite 0.5.0 (Qt Quick Markdown writer), omacalc 0.2.2 and omacut 0.4.0 contain `/.local/state/omarchy/current/theme/colors.toml` and `/.local/state/omarchy/current` (`strings -el`). omawrite's `src/backend.cpp` parses it line by line and watches `current/`, `theme/` and the file with `QFileSystemWatcher`, re-arming after each change. None uses a template or hook.
2. **The theme directory is replaced, not symlinked:** `rm -rf current/theme; mv next-theme theme`, then `theme.name` is rewritten. Watch the parent directory.
3. **Fonts come from fontconfig** (the `monospace` alias). No state file records the font; gsettings `monospace-font-name` stays `'Adwaita Mono 11'`.
4. **GTK and Qt apps get only dark/light from Omarchy**, never the palette.
5. **App-ids starting with `org.omarchy.` or `TUI.` are tagged `terminal`**, and `SUPER+C/V` then send Ctrl+Insert / Shift+Insert.
6. Omarchy's `text/plain` default is `nvim.desktop`. `$EDITOR` is `omarchy-launch-editor --inline`, which starts GUI editors **detached**, so blocking uses such as `git commit` return immediately.

### Theme pipeline (`/usr/share/omarchy/bin/omarchy-theme-set`)

- Paths: `CURRENT_THEME_PATH=~/.local/state/omarchy/current/theme`, `NEXT_THEME_PATH=…/current/next-theme`; user themes in `~/.config/omarchy/themes`, built-ins in `$OMARCHY_PATH/themes` (22 themes: 17 dark, 5 light).
- Serialized with `flock` on `${XDG_RUNTIME_DIR:-/tmp}/omarchy-theme-set.lock`.
- Steps: rebuild `next-theme` (built-in, then user overlay; git-installed themes are staged and denied files dropped); generate `colors.toml` from `alacritty.toml` if missing; run `omarchy-theme-set-templates`; swap (`rm -rf` + `mv`); write `theme.name`; send the palette to the shell over IPC; run post-theme commands in parallel (terminal, hyprctl, btop, helix, GNOME, VS Code, Obsidian, …); `omarchy-hook theme-set`.
- `omarchy-theme-refresh` re-runs `omarchy-theme-set` with the current name and `OMARCHY_THEME_SKIP_BACKGROUND=1`, so `theme.name` is rewritten there too.

### Template rendering (`omarchy-theme-set-templates`)

- Templates: `~/.config/omarchy/themed/*.tpl` first (user wins), then `$OMARCHY_PATH/default/themed/*.tpl`; only run when `next-theme/colors.toml` exists.
- Output `next-theme/<name minus .tpl>`, **never overwriting an existing file**, which is the per-theme override mechanism.
- Variables: every key of `omarchy-theme-color --all`. Syntax `{{ key }}` (one space inside), plus `{{ key_strip }}`, `{{ key_rgb }}`, `{{ mix a b 30% }}` (and `_strip`/`_rgb`), `{{ hypr_gradient … }}`, `{{ gradient_start … }}`, `{{ shell_gradient … }}`. `shell.<section>.toml` files are spliced into `shell.toml`.
- Installed-theme safety: `alacritty.toml foot.ini ghostty.conf kitty.conf vscode.json` and any `*.lua` are denied for git-installed themes. The upstream staging test classifies generated files as denied or colour-only; only an *upstream* template would have to be registered, which Stet doesn't add.

### Resolved palette (`/usr/share/omarchy/bin/omarchy-theme-color`)

- Usage: `omarchy-theme-color [--file <colors.toml>] (--all | --raw | <key> [fallback])`; `--all` prints every resolved key, tab-separated, sorted by key.
- Canonical keys: `mode, accent, selection, selection_background, selection_foreground, muted, cursor, background, dark_background, darker_background, lighter_background, foreground, dark_foreground, light_foreground, bright_foreground, red, yellow, orange, green, cyan, blue, magenta, brown, bright_{red,yellow,green,cyan,blue,magenta}`. Aliases: `purple`/`bright_purple`, `color0`–`color15`, legacy `bg/fg/…`, `theme_type`.
- Derived values: `cursor = bright_foreground` (always); `selection_foreground` → `bright_foreground`; `orange` → `yellow`; `brown = mix(orange, #000, 50%)`; `dark_background = mix(bg, #000, 25%)`, `darker_background = mix(bg, #000, 50%)`; `bright_* = mix(base, #fff, 20%)`; `muted` → `color8` → `dark_foreground`.
- Mode precedence: `mode`, then `theme_type`, then a `light.mode` file, then luminance (R+G+B > 382 is light), then dark. Value charset `^[A-Za-z0-9#(),._+/% -]*$`.
- **TC-8 additions** the report and draft missed: `selection` → `selection_background` → `color8` → `color0` → `background`; `lighter_background` → `color0` → `background` (the current-line colour can then vanish, so clamp it with a mix); bright/light/dark foreground → `color15`/`color7`/`color8` → `foreground`; ANSI-only themes take background/foreground from `color0`/`color7`; **`accent` has no fallback**.
- **Plan decision:** call `omarchy-theme-color --file <path> --all` at runtime (about 8 ms) and keep a Rust cascade only outside Omarchy ([ADR-004](DECISIONS.md)). A check on 2026-09-30 of catppuccin-latte, flexoki-light, white and tokyo-night showed sensible `dark_background`/`darker_background` values in light and dark themes alike.

### Syntax mapping precedents

| Role | `helix.toml.tpl` | `vscode-theme.json.tpl` |
| --- | --- | --- |
| keyword | magenta (`keyword.control` italic) | bright_magenta |
| function | blue | blue |
| type | yellow | yellow |
| constant / number | yellow | orange |
| string | green (regexp, escape: magenta) | green |
| comment | muted, italic | muted, italic |
| operator / punctuation | cyan / muted | bright_blue |
| selection | `selection_background` + `selection_foreground` | `selection_background` + alpha |
| current line, ruler | `lighter_background` | — |
| matching bracket | yellow, bold | — |
| error / warning / info / hint | red / yellow / blue / cyan | — |

**Plan decision:** the GtkSourceView scheme follows `helix.toml.tpl`, but numbers and constants use `orange` (like the VS Code template), which falls back to `yellow` ([DESIGN.md](DESIGN.md#syntax-roles)).

### How an app should consume the theme

- **Recommended and adopted:** read `current/theme/colors.toml` directly through one provider layer (the omawrite convention; omarchy-shell's `Commons/Color.qml` does the same). No install-time side effects; works on first launch; falls back to the portal's `color-scheme` outside Omarchy.
- **Optional override:** a colour-only `current/theme/<app>.toml` shipped by a theme or rendered from a user template `~/.config/omarchy/themed/<app>.toml.tpl`.
- **Rejected:** shipping only a `.tpl` (packages can't install into `~/.config/omarchy/themed/`; `/usr/share/omarchy/default/themed/` belongs to the omarchy package; templates render only on the next theme set). A `theme-set.d` hook is unnecessary (hooks run after the swap).

### Live reload

```
~/.local/state/omarchy/current/        <- watch this directory, non-recursive
  theme/          real directory, replaced with rm -rf + mv
  theme.name      rewritten after the swap
  background      symlink
```

- The report suggested inotify `IN_MOVED_TO|IN_CREATE` on `theme` and `IN_CLOSE_WRITE|IN_MOVED_TO` on `theme.name`, with the `notify` crate. A watch on `theme/` itself dies with `IN_DELETE_SELF`/`IN_IGNORED`.
- **TC-7 correction:** the swap is `mv next-theme theme` *inside* the watched directory, which GFileMonitor with `WATCH_MOVES` reports as **RENAMED**, not MOVED_IN or CREATED. Trigger on `theme.name` CHANGES_DONE_HINT or RENAMED-to-`theme`; ignore `next-theme`.
- Use a lenient `key = "value"` parser if parsing ourselves; third-party files may not be strict TOML.

### Fonts

- `omarchy-font-set <name>` checks the font with `fc-list`, rewrites the terminal configs, writes `~/.config/fontconfig/fonts.conf` with a `monospace` → `<name>` rule (`prepend_first`, `strong`), restarts the shell and runs `omarchy-hook font-set`. The script calls fontconfig "the canonical source of truth".
- System default `/etc/fonts/conf.d/50-omarchy.conf` (omarchy-settings) maps `monospace` to JetBrainsMono Nerd Font. `omarchy-font-current` is `fc-match monospace -f '%{family}\n' | head -n1 | cut -d, -f1`.
- There is no Omarchy-wide monospace size (foot uses 9 pt; the shell uses `base-size = 12` px). gsettings `text-scaling-factor` exists; omawrite honours it.
- The shell resolves `fc-match -f "%{family[0]}" monospace` and watches `fonts.conf`. **Unverified:** whether a running GTK/Pango process notices fontconfig changes by itself, hence the explicit re-resolve.

### GTK and Qt theming on Omarchy

- `omarchy-theme-set-gnome` sets `color-scheme` to prefer-dark/prefer-light and `gtk-theme` to Adwaita(-dark), plus the icon theme (default Yaru-blue). The portal (`hyprland-portals.conf`: `default=hyprland;gtk`) reports `color-scheme` 1; `accent-color` is NotFound. `~/.config/gtk-{3,4}.0/settings.ini` only sets the cursor; there is no `gtk.css`.
- A GTK4 app that wants the palette must inject its own `GtkCssProvider` and generate a GtkSourceView scheme. TC confirmed the libadwaita CSS variables exist in 1.9.3 and recommended also overriding `--popover-bg-color`, `--dialog-bg-color`, `--card-bg-color`, `--sidebar-bg-color`, `--headerbar-shade-color`, the foreground colours and `--window-radius: 0`.
- Qt: `QT_QPA_PLATFORMTHEME=gtk3`, `QT_QPA_PLATFORM=wayland;xcb`; Kvantum removed by a migration.
- Default GUI apps: GTK4/libadwaita (nautilus 50.3.1, tensaku 0.29.0, pinta 3.1.2); GTK3 (evince, sushi, xournalpp, localsend, gnome-disk-utility, aether); Qt6/Quick (omawrite, omacalc, omacut, moonlight-qt, kdenlive, quickshell 0.3.1); Electron (obsidian 1.13.7).

### Desktop integration

- **Desktop file precedent** (`omawrite.desktop`): `Exec=omawrite %f`, `Categories=Office;WordProcessor;TextEditor;`, `MimeType=text/markdown;text/x-markdown;text/plain;`, `StartupWMClass=omawrite`. Name the file `<app-id>.desktop` so it matches the Wayland app-id.
- **Launcher:** walker and elephant are not installed in 4.0.4. The launcher is omarchy-shell's `services/AppLibrary.qml` (Quickshell `DesktopEntries`), launching with `uwsm-app -- gtk-launch <id>.desktop`. A standard `.desktop` file is all that's needed.
- **Omarchy menu:** user entries go in `~/.config/omarchy/extensions/omarchy-menu.jsonc` (user-owned, hot-reloaded). Fields: `icon, label, action, target, provider, aliases, description, when, checked`, plus `iconFont` and `title`. Providers are hard-coded in `shell/plugins/menu/Menu.qml`, so a JSON "recent files" provider can't be added in 4.0.4.
- **Hyprland Lua:** helpers in `default/hypr/helpers.lua`: `o.bind(keys, description, dispatcher, opts)` with `{omarchy=…}`, `{launch=…}`, `{launch=…, focus=…}`, `{tui=…}`, `{webapp=…}`; `o.window(match, rules)`; `hl.unbind`. User rules go in `~/.config/hypr/hyprland.lua` and `bindings.lua`. Every window gets tag `default-opacity` (`0.985 0.96`); portal dialogs float (`xdg-desktop-portal-gtk` tagged `floating-window`); `misc.focus_on_activate = true`.
- **SUPER bindings:** SUPER+anything is compositor-owned. Free SUPER+SHIFT letters in the defaults: H I J K L Q R T U V (T was re-checked free on this machine on 2026-09-30). `SUPER+SHIFT+N` runs `omarchy-launch-editor`; `SUPER+S` toggles the scratchpad.
- **Default editor:** `omarchy-launch-editor` reads `~/.local/state/omarchy/defaults/editor` (falls back to `nvim`); `nvim|vim|nano|micro|hx|helix|fresh` run in a terminal (inline with `--inline`); anything else runs `exec setsid uwsm-app -- "$editor" "$@"`. `omarchy-default-editor` only accepts `code|cursor|zed|sublime_text|helix|vim|emacs|nvim`.
- **MIME:** don't touch `/usr/share/applications/mimeapps.list` (omarchy-settings); user overrides go to `~/.config/mimeapps.list` via `xdg-mime default`.
- **Terminal:** `TERMINAL=xdg-terminal-exec` (foot preferred); "open terminal here" is `setsid uwsm-app -- xdg-terminal-exec --dir=<dir>`.

Details and snippets: [INTEGRATIONS.md](INTEGRATIONS.md).

### omarchy-shell plugins

Manifest fields `schemaVersion: 1, id, name, version, author, description, kinds[], entryPoints{}` plus optional `barWidget`, `keepLoaded`, `activation`; kinds `bar-widget | panel | overlay | menu | service | bar`; installed in `~/.config/omarchy/plugins/<id>/`; unsandboxed; hot-reloaded. The user's convention is `io.github.pixdevsapps.<name>` (for example `hertz-radio`). Ideas: a quick-note panel, a recent-files panel reading a JSON file the app writes. **Plan decision:** 1.x at the earliest.

### Packaging pattern

- **Hertz:** `tools/package-source.py` builds a deterministic tarball and fills `@SHA256@` and `@SOURCE_DATE_EPOCH@`; `prepare` `cargo fetch --locked`; `build` `cargo build --release --locked --offline`; `check` `cargo test --workspace --locked --offline`; `options=('!lto')`; installs binary, desktop file, hicolor icons, licenses (`THIRD_PARTY.md`, `dependency-licenses.json`) and docs.
- **omawrite** (Omarchy first-party): builds in-tree, installs binary, license, scalable icon and desktop file; distributed from `[omarchy] https://pkgs.omarchy.org/stable/$arch`.
- The Arch "Rust package guidelines" page was unreachable (bot protection), so its current recommendations are unverified. Follow Hertz.

---

## 3. Rust GUI toolkits

### Versions (crates.io API unless noted)

| Component | Latest | Date | Notes |
| --- | --- | --- | --- |
| gtk4 | 0.11.5 | 2026-09-20 | MIT; features up to `v4_22`; system GTK 4.22.4 |
| libadwaita (crate) | 0.9.2 | 2026-07-07 | MIT; features up to `v1_10`; system 1.9.3 |
| sourceview5 | 0.11.2 | 2026-07-23 | MIT; `v5_2`…`v5_18`, `v5_22` (no `v5_20`) |
| GtkSourceView (C) | 5.22.0 | 2026-09-18 | LGPL; Arch has 5.20.0 (2026-03-16) |
| relm4 | 0.11.0 | 2026-04-08 | not used |
| iced | 0.14.0 | 2025-12-07 | no 0.15 yet |
| cosmic-text | 0.19.0 | 2026-04-22 | libcosmic is git-only |
| gpui / gpui-pre | 0.2.2 (frozen) / 0.3.7 | 2025-10-22 / 2026-09-28 | `gpui-ce` 0.3.x yanked 2026-08-28 |
| gpui-component | 0.7.0 | 2026-09-28 | repo now longbridge/gpui-kit (~15k stars) |
| cxx-qt | 0.10.0 | 2026-08-24 | Hertz's bridge |
| egui / slint | 0.36.2 / 1.18.1 | 2026-09 | slint GPL-3.0 or royalty-free |
| floem / makepad-widgets / xilem | 0.2.0 / 1.0.0 / 0.4.0 | 2024-11 / 2025-05 / 2025-10 | stale or experimental |
| winit | 0.30.13 / 0.31.0-beta.3 | 2026-03 / 2026-09 | Wayland file DnD only in 0.31 |

### Checks run on this machine (PyGObject against GtkSourceView 5.20)

- 180 language IDs, bundled in the library as GResources (so `/usr/share/gtksourceview-5/language-specs` holds only schemas). The language search path includes `~/.local/share/gtksourceview-5/language-specs`, so user `.lang` files work (the equivalent of user-defined languages).
- 12 built-in style schemes; style search path includes `~/.local/share/gtksourceview-5/styles`.
- FileLoader's default candidates: `UTF-8`, `CURRENT`, `ISO-8859-15`, `UTF-16`; `Encoding.get_all()` returns 62.
- On 2026-09-30 the GtkSourceView 5.20 library was also checked for the style ids used in [DESIGN.md](DESIGN.md#syntax-roles) (`def:keyword` … `def:operator`, `def:special-char`, `def:error`) and scheme styles (`text`, `selection`, `cursor`, `current-line`, `line-numbers`, `draw-spaces`, `bracket-match`, `search-match`, …).

### A. gtk4-rs + libadwaita + sourceview5 (chosen)

- **Maturity:** the most mature Rust GUI binding. sourceview5 has had 14 releases. **Maintainer risk:** Christian Hergert wrote in February 2026 that his "direct involvement going forward will be very limited"; he still cut 5.22.0 and committed fixes in September 2026, and GtkSourceView is GNOME core (Text Editor and Builder depend on it).
- **Free:** 180 languages; runtime style schemes (`append_search_path` + `force_rescan`); line numbers, marks, right margin; auto-indent, smart backspace/home-end, bracket matching, `move-lines`/`move-words`/`join-lines`/`change-case` signals, snippets, annotations (5.18); `GtkSourceMap`; `SearchContext` (regex, case, word boundaries, wrap, highlight-all, replace-all); `SpaceDrawer`; wrap; undo with user-action grouping; completion (`CompletionWords`); printing; tabs (AdwTabView/AdwTabBar with DnD between windows); GtkPaned (two views can share one buffer: "clone to other view"); GMenuModel; shortcut controllers; `GtkDropTarget`; GFileMonitor; AT-SPI.
- **Build ourselves:** multi-cursor (GNOME Text Editor #253 says it needs "deep changes"), column selection, folding, statistical encoding detection, macros, hex, compare, Find in Files. Paths: column mode via `snapshot_layer` painting and per-line edits in one user action; folding via invisible tags plus a gutter renderer (TC-10: tags affect every view of the buffer); multi-caret via extra marks, the hardest and most fragile.
- **Large files, the weak point:** a line B-tree (5.22 removed per-line allocations); GNOME Text Editor freeze reports at 2 MB+ (#391) and a truncation bug near 800 kB (#645); very long lines are pathological because Pango lays out whole paragraphs (GTK #229). Performance at 100 MB was estimated, not measured, hence spike S1.
- **Wayland:** fractional scaling since 4.14, text-input-v3, native clipboard and DnD. GTK draws its own header; Hyprland draws no title bars.
- **Theming:** generated scheme XML plus a CSS provider; reload on `current/` changes.

### B–I in brief

- **B. iced 0.14 / cosmic-text / libcosmic:** iced's `text_editor` lacks a gutter, search, multi-cursor, folding and minimap; high-CPU issue #2477 is fixed only for the unreleased 0.15. cosmic-text selections have no block or multi-cursor mode. COSMIC Edit has no session restore (#134), memory problems on big files (#182) and a `//TODO` Encoding menu. winit 0.30 has no Wayland file DnD (#1881).
- **C. GPUI + gpui-component (fallback):** tree-sitter highlighting, folding, multi-cursor with column selection, search/replace, LSP; claims "stable performance at 200K lines". Costs: distribution churn (frozen `gpui`, weekly `gpui-pre`, yanked `gpui-ce` broke downstream builds, runner #733), Vulkan GPU, fractional-scaling blur reports (Zed #33464, #25195), a no-window report on Hyprland + RADV (#37918), accessibility in progress, no native menus or printing. Regex search, soft wrap, minimap and encodings are undocumented.
- **D. CXX-Qt 0.10 + QML (Hertz stack):** mature Qt on Wayland and reusable Hertz theme code, but QML TextEdit has no gutter, minimap, search UI, block selection or folding; `QSyntaxHighlighter` highlights the whole document; cxx-qt-lib lacks `QTextDocument` bindings. QScintilla and KTextEditor are Qt Widgets, which CXX-Qt deliberately does not bind (`qt_widgets` 0.5.0 from 2020 is dead), so the UI would be mostly C++.
- **E. egui 0.36.2:** re-lays out the whole text on change; no Wayland file DnD (egui #1563). **F. Slint 1.18.1:** plain `TextEdit` only. **G. Floem:** last release November 2024. **H. Makepad 1.0:** Wayland backend recently refactored; its editor isn't documented for reuse. **I. Xilem 0.4:** experimental.
- **Pure-Rust editor core** (rope, tree-sitter, encoding_rs): best large-file design, but reaching GtkSourceView's IME, accessibility, bidi, completion and printing would take many months.

### Scored comparison (weights: editor 25, large files 10, Wayland 15, theming 10, app shell 10, maturity 15, effort 10, stack fit 5)

| Option | Editor | Large | Wayland | Theme | Shell | Maturity | Effort | Fit | **Total** |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| **A. GTK4 + libadwaita + sourceview5** | 4 | 2.5 | 5 | 4.5 | 5 | 4 | 5 | 3 | **84** |
| D′. Qt Widgets + QScintilla (C++ UI) | 5 | 3.5 | 4.5 | 4 | 4.5 | 3.5 | 2.5 | 2 | 80 |
| **C. GPUI + gpui-kit** | 4.5 | 4 | 3 | 5 | 3.5 | 2 | 3.5 | 2 | **71.5** |
| D. CXX-Qt + QML + KSyntaxHighlighting | 2 | 2 | 4.5 | 4.5 | 4 | 4 | 2.5 | 5 | 66.5 |
| G. Floem | 3 | 3 | 2.5 | 4 | 2 | 1.5 | 2.5 | 2 | 52 |
| B. iced / cosmic-text | 1.5 | 2 | 3 | 5 | 2.5 | 3 | 2 | 3 | 51.5 |
| E. egui | 1.5 | 1 | 2.5 | 5 | 2.5 | 4 | 2.5 | 2 | 51 |
| F. Slint | 1 | 1.5 | 3.5 | 4 | 3 | 4 | 1.5 | 3 | 50.5 |
| H. Makepad | 2.5 | 3 | 2 | 4 | 2 | 2 | 2 | 2 | 48.5 |
| I. Xilem | 1 | 1.5 | 2.5 | 4 | 1.5 | 1.5 | 1 | 2 | 35 |

The suggested A-vs-C spike (100 MB and 10 MB single-line files, typing latency, regex replace-all over 1M lines, fractional scaling and fcitx5, Nautilus DnD, theme hot reload) became the M0 spikes S1–S6. **Plan decision:** [ADR-002](DECISIONS.md).

---

## 4. Core crates

The crates report assumed, for most of its recommendations, that the Rust core owns the buffer. With GtkSourceView the toolkit owns buffer, undo, highlighting and in-document search, so several recommendations were overridden.

### Buffer and undo

- Recommended if the core owns the text: `ropey` 1.6.1 (`default-features = false`, `simd` + `cr_lines`, so line breaks match Scintilla rather than all Unicode breaks), O(1) clone for snapshots, distinct `CharIdx`/`ByteIdx`/`Utf16Idx` types, and a Helix-style undo (change sets, transactions, linear history, typing grouped by kind/adjacency/time, a save point for the dirty flag). Rejected alternatives: ropey 2.0 beta, crop (no CR-only lines), jumprope, xi-rope.
- **Plan decision:** no rope; the GtkSourceBuffer is the source of truth and domain operations return `TextEdit`s ([ADR-003](DECISIONS.md)). A ropey mirror is the fallback if S1 shows 50 MB snapshots cost more than 100 ms.

### Syntax highlighting

- Recommended for a Rust-owned buffer: `syntect` 5.3.0 + `two-face` 0.5.2 (150+ Sublime syntaxes; onig backend needs `oniguruma`; bincode advisory RUSTSEC-2025-0141), with incremental re-highlighting via cached parse states; `tree-sitter` later for folding and a function list.
- tree-sitter: pin **0.26** (0.26.13), because `tree-sitter` declares `links = "tree-sitter"` and `tree-sitter-md` 0.5.3 and `tree-sitter-perl` 1.1.2 need `^0.26`. `tree-sitter-highlight` re-parses the whole file on every call; incremental highlighting needs our own tree maintenance and viewport queries. No log grammar exists. Bundles: `arborium` (own fork), `tree-sitter-language-pack` (downloads parsers), `tree-house` (MPL-2.0, unstable API).
- **Plan decision:** GtkSourceView highlights (180 languages); tree-sitter function list and structural folding are Later.

### Encodings

- Crates: `encoding_rs` 0.8.42 (`(Apache-2.0 OR MIT) AND BSD-3-Clause`), `chardetng` 1.0.0 (`EncodingDetector::new`, `feed`, `guess`), `oem_cp` 2.1.2 (DOS code pages), `codepage` 0.1.3.
- Gaps: OEM code pages (`oem_cp`); true ISO-8859-1 (the WHATWG label maps to windows-1252; use `encoding_rs::mem` Latin-1 helpers); UTF-7/UTF-32 unsupported.
- Detection order: BOM (`Encoding::for_bom`) → valid UTF-8 → NUL pattern in the first 8 KiB (UTF-16 without BOM, else binary) → `chardetng` with UTF-8 denied → per-file user override. chardetng never guesses UTF-16.
- **Save pitfalls:** encoding_rs has **no UTF-16 encoder**; `Encoding::encode` silently writes HTML numeric references for unrepresentable characters; use `encode_from_utf8_without_replacement` and ask. Decoding errors leave U+FFFD, so block silent saving.
- Binary files: decode as isomorphic Latin-1 so bytes round-trip; open read-only.
- Reinterpret ("Encode in") reloads bytes with another encoding; Convert changes the encoding used on the next save.
- Line endings: count CRLF/LF/CR with `memchr`, majority wins (LF on a tie), "Mixed" status. The report suggested storing text verbatim.
- **Plan decisions:** own pipeline with the NUL placeholder and `lossy_roundtrip` flag ([ADR-007](DECISIONS.md), TC-2, TC-14); LF-normalized buffer ([ADR-008](DECISIONS.md)).

### Search

- Recommended: `fancy-regex` 0.19.2 (delegates simple patterns to `regex` 1.13.1; lookaround, backreferences, atomic groups; set a backtrack limit), `\R` rewritten, Extended-mode unescaper (`\n \r \t \0 \\ \xHH \oOOO \dDDD \bBBBBBBBB \uHHHH`), whole word checked manually, own replacement-template parser (`$0 $& $1..$99 ${n} ${name} \0..\9`, escapes, Boost `\U \L \E \u \l`), replace-all as one change set, and an `Arc<str>` snapshot per revision. Alternative for Boost parity: `grep-pcre2` 0.1.10 / `pcre2` 0.2.11 with PCRE2 10.48 installed.
- **Plan decision:** PCRE2 everywhere ([ADR-005](DECISIONS.md)): GtkSourceView already uses PCRE2 with JIT (linked directly, not via GRegex; TC low), so fancy-regex would create a second dialect. `\R` is native in PCRE2; Boost `\<` `\>` need translation; the `pcre2` crate has no substitute API, so the template parser is still ours.

### Find in Files

| Find in Files option | `ignore` 0.4.33 `WalkBuilder` |
| --- | --- |
| In hidden folders | `.hidden(!in_hidden)` |
| In all sub-folders unchecked | `.max_depth(Some(1))` |
| Symlinks | `.follow_links(false)` |
| Respect .gitignore (new; default on) | `.git_ignore()`, `.ignore()`, `.parents()` |
| Filters with `!` excludes | `OverrideBuilder` |

- Search each file with `grep-searcher` 0.1.17 (line numbers, multi-line, `BinaryDetection::quit(b'\0')`, BOM sniffing, mmap internally); walk with `build_parallel()`.
- Stream per-file batches over a bounded channel drained at ~30 Hz; cap at ~50k hits; cancel through `Arc<AtomicBool>` (`WalkState::Quit`, sink returns `Ok(false)`); tag batches with a generation id.
- Replace in Files: decode, replace, re-encode with the same encoding, BOM and EOL, safe-save; skip binary and oversized files; open documents get an undoable edit instead.
- Find All in open documents searches in-memory snapshots, separately from Find in Files (which reads disk).
- **Plan decision:** `grep-pcre2` for Regex mode, `grep-regex` fixed strings for Normal/Extended.

### File monitoring and safe save

- The report recommended `notify` 8.2 + `notify-debouncer-full` 0.7 watching parent directories. **Plan decision:** GFileMonitor already watches the parent directory, so rename-over saves arrive as RENAMED or MOVED_IN (TC low); plus a re-stat when the window regains focus (network filesystems may emit nothing).
- Fingerprint `(dev, ino, size, mtime_ns)`; record it right after our own save to ignore our write.
- Reload a clean file as one undoable step keeping the caret; dirty: Reload / Keep mine / Compare; deleted: keep the buffer, mark it modified.
- Safe save: canonicalize (keep symlinks), temp file in the same directory, fsync, rename, fsync the directory, keep mode and owner (`fchown` where allowed). Hard links or an unwritable directory: back up, then overwrite in place. Rename drops xattrs/ACLs (`xattr` crate only if needed). `atomic-write-file` 0.3.1 replaces symlinks unless the path is canonicalized.
- Root-owned files: the report proposed a `pkexec` helper binary with a polkit `auth_admin_keep` action. **Plan decision:** MVP shows a read-only banner pointing to `sudoedit` with `--wait`; the helper is 1.x (SC-M6).
- Never back an editable buffer with `memmap2`: truncation by another process causes SIGBUS.
- Size policy in the report: large mode above ~200 MB, refuse above ~2 GB or half of free RAM. **Plan decision:** large mode at 50 MB, cap at 256 MB or when free RAM is below 4× the file size (TC-15).

### Session and backups

- Plain files under `$XDG_STATE_HOME/<app>/` via `directories` 6.0: `session.json` (versioned, `#[serde(default)]`), `.prev`, per-document backups, a lock (`fs4` 1.1), 0700/0600 permissions.
- Per-tab fields: id (uuid v7), path, display name, dirty, backup file, read-only, encoding/BOM/EOL/language overrides, carets and selections, first visible line, disk fingerprint; plus tab order, active tab, geometry, recent files, find history.
- Loop: mark on edit; every 5–7 s snapshot marked documents; write backups (temp, fsync, rename) **then** the manifest; delete a backup on clean save or discard; flush synchronously on quit. Restore falls back to `.prev`; conflicts offer Compare; unreferenced backups are kept, not deleted.
- SQLite (`rusqlite` 0.40.2, as Hertz) would also work but rewrites multi-MB blobs every few seconds and is harder to recover by hand. **Plan decision:** [ADR-006](DECISIONS.md).

### Other crates

| Need | Crate (version, license) | Note |
| --- | --- | --- |
| Config | `serde` 1.0.229, `toml` 1.1.6 | `toml_edit` 0.25 only with a Preferences GUI (1.x) |
| Clipboard | toolkit clipboard | Wayland clipboard needs a focused window |
| Compare | `similar` 3.2.0 (Apache-2.0, `inline` + `unicode`); `imara-diff` 0.2.0 | Patience with timeout; word-level highlights |
| JSON | `serde_json` 1.0.151 | **TC-9:** never `arbitrary_precision` (breaks untagged/flatten types binary-wide; pretty-printing via `Value` drops duplicate keys and rewrites escapes). Token-level formatter; validate with `IgnoredAny` for line/column |
| XML | `quick-xml` 0.42.0, `roxmltree` 0.21.1 | pass comments, CDATA, PIs, DOCTYPE through; don't re-indent mixed content or `xml:space="preserve"` |
| Hash/encode | `sha2` 0.11, `base64` 0.23.1, `percent-encoding` 2.3.2 | not planned |
| Spell check | `spellbook` 0.4.2 (MPL-2.0, alpha) | Later; no hunspell dictionaries installed |
| CLI | `clap` 4.6.7 (derive) | `file:line[:col]` only when the literal path doesn't exist; TC-4 on `try_get_matches_from` |
| Single instance | GApplication (D-Bus) | `zbus` 5.19 only for Qt |
| Logging | `tracing` 0.1.44, `tracing-subscriber` 0.3.23 | never log document content |
| Threads | `async-channel` 2.5, `crossbeam-channel` 0.5.17 | glib futures drain `async-channel` |
| Testing | `proptest` 1.11, `insta` 1.48, `tempfile` 3.27, `criterion` 0.8.2 | `cargo-deny`/`cargo-about` not installed (SC-L1) |

### Existing Rust editors and licensing

- **Helix** (MPL-2.0): `helix-core` is not published; re-implement its design rather than depend on it. Reusable published crates: `tree-house`, `spellbook` (MPL-2.0), `regex-cursor` (MIT/Apache), `imara-diff` (Apache-2.0).
- **Zed:** `rope`, `text`, `language`, `multi_buffer` are GPL-3.0-or-later, incompatible with an MIT app.
- **Lapce** (Apache-2.0): heavy Delta/Engine model. **COSMIC Edit** (GPL-3.0-only): don't copy app code.
- For an MIT app: MIT, Apache-2.0, BSD, ISC, Zlib, Unlicense and CC0 are fine (ship notices; note encoding_rs's BSD-3-Clause data); MPL-2.0 is fine as an unmodified dependency; avoid GPL/LGPL crates such as `chardet` 0.2.

### Planned dependency set (approved plan)

```toml
gtk4 = { version = "0.11.5", features = ["v4_18"] }; libadwaita = { version = "0.9.2", features = ["v1_9"] }
sourceview5 = { version = "0.11.2", features = ["v5_18"] }; async-channel = "2.5"; clap = { version = "4.6", features = ["derive"] }
serde = { version = "1", features = ["derive"] }; serde_json = "1"   # no arbitrary_precision
quick-xml = "0.42"; roxmltree = "0.21"; memchr = "2.8"; unicode-segmentation = "1.13"; thiserror = "2"
uuid = { version = "1.26", features = ["v7", "serde"] }
encoding_rs = "0.8.42"; chardetng = "1.0"; ignore = "0.4.33"; grep-searcher = "0.1.17"; grep-regex = "0.1.14"
grep-pcre2 = "0.1.10"; grep-matcher = "0.1.9"; pcre2 = "0.2.11"; tempfile = "3.27"; fs4 = "1.1"; directories = "6.0"
toml = "1.1.6"; crossbeam-channel = "0.5.17"; tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter"] }; anyhow = "1"
similar = { version = "3.2", features = ["inline", "unicode"] }   # 1.0 compare
# dev: proptest = "1.11"; insta = { version = "1.48", features = ["json"] }; criterion = "0.8.2"
```

Deliberately not used: syntect/two-face, ropey, fancy-regex, notify, zbus, ashpd, toml_edit ([BUILD.md](BUILD.md#dependency-rules)). `Cargo.toml` is the source of truth for what is actually in the build.

---

## 5. Prior art

### Summary

1. Nothing Omarchy-native fills the gap: out of the box there is nvim (default, terminal), Omawrite (Markdown, one document) and Obsidian, plus an install menu for VS Code, Cursor, Zed, Sublime, Helix, Vim and Emacs.
2. No mature Rust editor of this kind (a fast, keyboard-first GUI text editor) exists; Rust clones of simple text editors have under 20 stars.
3. Omarchy users are asking (all 2026).

### Demand signals on Omarchy

| Signal | Date | Takeaway |
| --- | --- | --- |
| Discussion #13697 "How to set Kate as default text editor" | 2026-09-28 | no GUI notepad path |
| Discussions #8360 (+5), #10960: add nano as default editor | 2026-08/09 | demand for a simple non-modal editor |
| Issue #7810: default editor doesn't update XDG MIME | 2026-08-22 | the app must register as MIME handler |
| Issue #13037 / PR #13094: `--inline` passes no wait flag to GUI editors | 2026-09-23 | `--wait` and single-instance forwarding |
| PR #12451 native Quick Notes scratchpad | 2026-09-18 | fast capture without a full window |
| Discussion #13399: native "keyboard-first, theme-synced" file browser | 2026-09-27 | Omacalc/Omawrite are the reference style |

Community notepads: **OmNote** (Python, GTK4/GtkSourceView5, tabs, session, live theme; MIT; 13 stars), **omarchy-notepad** (QML on Quickshell; 0 stars), **OmaText** (no tabs, no autosave, 2 MiB cap; 1 stars), plus omarchy-working-memory and omarchy-scratchpad. They show the need, not a solution. Reddit could not be searched; the evidence is GitHub-only.

### Omawrite, the pattern to copy

Reads `colors.toml` (`mode`, `background`, `foreground`, `accent`, `selection`) with a `QFileSystemWatcher`; falls back to hard-coded colours and the portal's `color-scheme`; follows `text-scaling-factor`; bundles iA Writer Mono; has draft recovery, external-change detection, in-window dialogs, no menu bar, shortcuts for everything. Omarchy's Hyprland look is square (`rounding = 0` by default; this machine overrides it to 8).

### Comparison

| Editor | Toolkit | License | Status (verified) | Strengths | Gaps relevant to us |
| --- | --- | --- | --- | --- | --- |
| Kate | KF6 KTextEditor | LGPL/GPL | 26.08.1 | block select, multi-cursor, folding, stash | KF6 dependency tree; ignores palette |
| GNOME Text Editor | GTK4/GtkSourceView5 | GPL-3.0 | 51.0 (Arch 50.1) | best drafts folder | no Find in Files, column, multi-cursor; Adwaita only |
| gedit / Xed / Mousepad | GTK3 | GPL-2.0 | 50.0 / 3.8.9 / 0.7.0 | light; Mousepad restores sessions | no Find in Files or column mode |
| Geany | GTK3 + Scintilla | GPL-2.0 | 2.1.0 | fast, rectangular select, grep | IDE-style chrome |
| FeatherPad | Qt6 | GPL-3.0 | 1.6.4 | sessions, column select | no Find in Files |
| Sublime Text | custom | proprietary | build 4215 | excellent, `hot_exit` | paid; already an Omarchy option |
| Zed | Rust/GPUI | GPL/AGPL/Apache | 1.22.0, 2026-09-30 | `restore_unsaved_buffers` | code editor; 5 GB freeze (#64232); Vulkan |
| Lapce | Rust/Floem | Apache-2.0 | 0.4.6 | LSP | code editor |
| COSMIC Edit | Rust/libcosmic | GPL-3.0 | epoch-1.9.0 | project search | no session restore (#134), memory (#182), crashes (#457), `//TODO` menus |
| Ferrite | Rust/egui + ropey | MIT | 0.3.0 | 1.8k stars | "100% AI-generated"; Wayland keyboard bug |

### What to copy

Never lose work (drafts, snapshots, `hot_exit`, `restore_unsaved_buffers`); tabs in one window with single-instance forwarding (COSMIC #517, #261); Find in Files with regex (COSMIC uses grep/ignore crates); large-file mode with progress and a warning instead of a hang (COSMIC #243, Zed #64232); theme on day one; careful encodings and EOL shown and clickable in the status bar; the everyday utilities; the classic editor keymap's muscle memory without clashing with SUPER; Omarchy behaviour (`--wait`, MIME registration, stable app-id, text scaling, `SUPER+F`).

### What to avoid

Floating dock panels on Wayland (Qt-ADS can force a Qt app onto XWayland); classic desktop chrome (menu bar, toolbar, modal dialogs); early scripting or plugins (a security surface); a web engine; loading whole files into rich structures up front; menu items that do nothing (COSMIC `Action::Todo`); relying on GTK/Qt platform theming; untested toolkit input paths (Ferrite's Wayland keyboard bug); going too minimal (OmaText, GNOME Text Editor).

### Positioning

> The features power users actually use (tabs that never lose work, regex and Find in Files, column editing, encoding and line-ending control, a safe large-file mode), in a keyboard-first Omarchy app that follows the live theme. A sibling to Omawrite and Omacalc, written in Rust.

Differentiation: native to Omarchy, reliability and focus, a Rust codebase. See [PRD.md](PRD.md#positioning).

---

## 6. Critique corrections

### Technical feasibility review

| # | Severity | Finding | Correction | Landed in |
| --- | --- | --- | --- | --- |
| 1 | High | `--wait` only blocks when a primary already runs; otherwise git becomes the primary, inherits its env/cwd, and dies with the terminal | D-Bus service file, `DBusActivatable`, `StartServiceByName` first; exit 1 on unsaved close; exclude from session | ADR-011, M2 |
| 2 | High | GtkTextBuffer silently drops a whole insert containing NUL | NUL ↔ ␀ placeholder; check every chunk insert; fixture | ADR-007, M3 |
| 3 | High | A long-line guard that still inserts the raw line doesn't avoid Pango's full-paragraph layout | Never insert raw long lines: open formatted, read-only with display breaks, or open anyway | S1, M3 |
| 4 | Medium | clap `get_matches_from` can `process::exit` the primary | parse locally before `run()`; `try_get_matches_from` in the primary | ADR-011 |
| 5 | Medium | GtkSourceView/GtkTextView/AdwTabView built-in bindings clash with the keymap | editor-scoped capture-phase overrides; reuse `move-to-matching-bracket` | ADR-009 |
| 6 | Medium | fcitx5 grabs both Ctrl+Shift+U and Ctrl+Alt+Shift+U | another UPPERCASE key; document `unicode.conf` | ADR-009, INTEGRATIONS |
| 7 | Medium | Theme swap is RENAMED, not MOVED_IN/CREATED | trigger on `theme.name` or RENAMED-to-`theme` | ADR-004 |
| 8 | Medium | Rust fallback cascade incomplete | call `omarchy-theme-color --all`; Rust cascade only outside Omarchy; own accent | ADR-004 |
| 9 | Medium | serde_json `arbitrary_precision` breaks other types; `Value` rewrites content | token-level JSON tools | BUILD dependency rules, M5 |
| 10 | Medium | Folding via tags affects every view of the buffer | accept with an ADR or block split while folded; spike first | folding moved to 1.x |
| 10a | Medium | Column typing arrives via the IM commit path; S5 didn't test it | S5 pass bar includes fcitx5 typing, Backspace, paste, undo; custom clipboard MIME | S5, M6 |
| 11 | Medium | 1M small edits = 1M mutations and undo entries | bulk replace-and-remap above a threshold; cap undo | ADR-003 |
| 12 | Medium | Mark and style tokens shouldn't be live search contexts | static GtkTextTags; named scheme styles re-applied on swap | ADR-004, M7 |
| 13 | Medium | No SIGTERM/SIGHUP handling (uwsm stops scopes with SIGTERM) | `unix_signal_add_local` flushes and quits | ADR-011, M2 |
| 14 | Medium | Byte-for-byte round-trip can't pass for Shift_JIS/GB18030 or bad UTF-16 | `lossy_roundtrip` flag; "round-trips or flagged lossy" | ADR-007, M3 |
| 15 | Medium | Large-file policy too loose; 1 GB editable unrealistic | disable wrap, map, space drawer, bracket match, highlights; non-editable while loading; 256 MB cap, 4× free RAM | M3 large-file mode |
| 16 | Medium | Go/no-go required both S1 and S5 to fail | judge each separately | PLAN M0 |
| 17 | Medium | Milestones undersized | M0 8–12 d; resized plan | PLAN |
| — | Low | Search doesn't go through GRegex; `\R` native; `pcre2` crate has no substitute; AdwShortcutsDialog since 1.8; CSS variables present in 1.9.3; portals default since GTK 4.18 (drop `ashpd`); `focus_on_activate` already set; `text/x-rust`/`text/x-toml` don't exist; `--workspace` for helper crates; polkit annotation; `check()` can't run GTK tests; list `pcre2` in depends; GFileMonitor watches the parent; save point clears `modified`; `.Devel` app-id for debug | folded into ADRs, BUILD and INTEGRATIONS | various |

Verified correct by the review: crate versions and features (`sourceview5` 0.11.2 has `v5_18` and `v5_22`, no `v5_20`; `gtk4` 0.11.5 has `TextViewImpl::snapshot_layer`; chardetng 1.0 API; `ignore` 0.4.33; `grep-pcre2` 0.1.10; `toml` 1.1.6); 22 built-in themes with `mode`/`accent`/`selection`/`lighter_background`; `SUPER+SHIFT+T` free; `omarchy-launch-editor` detaches GUI editors.

### Scope and user-value review

| # | Finding | Correction |
| --- | --- | --- |
| H1 | 1.0 drifted toward full parity (+28–40 days of less-used features) | lean 1.0; everything else to 1.x ([ADR-013](DECISIONS.md)) |
| H2 | Column mode came too late | prefix/suffix and numbering in the MVP; column mode first after it (M6) |
| H3 | Nothing usable until M4; no design approval | installable package in M1; DESIGN mock gate before UI work; a preview gate at every milestone |
| H4 | Theme landed late though the user reversed it on Hertz | ask first (user chose to follow the theme); theme and font in M1 |
| M1 | Find All / Replace All in open documents missing | added to the MVP (M4) |
| M2 | Search milestone too big | split into search (0.2) and toolbox (0.3) |
| M3 | Never-lose-work waited behind file edge cases | M2 sessions (UTF-8 only), M3 files |
| M4 | Too keyboard-only | tab and editor context menus, window title |
| M5 | Indentation settings missing | per-language settings, detection, status-bar item |
| M6 | pkexec helper is a large security surface | `sudoedit` + `--wait`; helper 1.x |
| M7 | Preferences GUI doesn't fit Omarchy | palette opens `config.toml`/`keys.toml`, hot reload |
| M8 | The name must be decided early | name and app-id are an M0 exit criterion ([ADR-010](DECISIONS.md)) |
| M9 | Hertz conventions missing (Goal/Key outcomes/Exit criteria, OPERATING, authorization rule, git init) | adopted in these docs |
| L1 | cargo-deny/cargo-about claim was wrong | reuse Hertz's `license-report.py`; cargo-deny optional, needs approval |
| L2 | Document map had the weakest evidence | off-by-default toggle, disabled in large mode |
| L3 | M0 overloaded | self-test harness moved to M1; only M0-relevant ADRs drafted first |
| L4–L7 | Never list additions; opt-in default-editor command; GPUI trigger; no GTK self-test in `check()` | adopted |

---

## 7. Open and unverified items

| Item | Status | Settled by |
| --- | --- | --- |
| GtkTextView performance at 100 MB / 1M lines, 10 MB typing, long-line path, snapshot cost | measured headless 2026-09-30 (mixed); Hyprland keystroke-to-paint not measured; highlighting policy approved (ADR-014), its threshold measured in M3 | S1 ([results](#m0-spike-results)) |
| Regex replace-all over 1M lines as one undo step; `\U$1` | measured headless 2026-09-30 (one step; 8.0 s fails ≤ 5 s; `\U$1` literal); replacing moved to our own engine (ADR-005 amendment) | S2 ([results](#m0-spike-results)), M4 |
| libadwaita CSS variables recolour the whole UI; live scheme reload across themes | measured headless 2026-09-30 (pass for sampled surfaces); reload trigger amended (ADR-004); live Hyprland check by eye pending | S3 ([results](#m0-spike-results)) |
| Fractional scaling, fcitx5 typing, key reachability (incl. Ctrl+Shift+U), DnD, dialog floating, activation focus, `SUPER+C/V` | not tested | S4 (live session) |
| Column painting and IME-path replication with single-step undo | measured headless 2026-09-30 (feasible; fixes approved in ADR-015, ADR-016 and the ADR-003 amendment); fcitx5 not tested | S5 ([results](#m0-spike-results)) |
| Cold start; D-Bus-activated `--wait` when not running; dev builds without a service file | warm start measured headless 2026-09-30; true cold start, `--wait` and service-less dev builds not measured | S6 ([results](#m0-spike-results)), M2 |
| Final product name; availability | decided 2026-09-30: **Stet** (ADR-010). No `stet` package in the Arch repositories or the AUR and no `stet` command on the system; crates.io has an unrelated `stet` crate (ours are `publish = false`). Trademarks and GitHub names not checked | ADR-010 |
| GTK/Pango picking up fontconfig changes in a running process | unverified; explicit re-resolve planned | M1 |
| GTK honouring `text-scaling-factor` through the portal | unverified | M1 |
| fcitx5 `DirectUnicodeMode` option name in the installed binary | upstream name, not confirmed locally | M5 user guide |
| Arch Rust packaging guideline details | page unreachable | M1 packaging |
| Omarchy menu custom providers | local `Menu.qml` says no | informational |
| Reddit r/omarchy sentiment | not searchable | informational |

---

## M0 spike results

Spike results are recorded in `docs/spikes/`, one file per spike, each with the date, exact command, machine and library versions, raw numbers, the pass bar and caveats (for example "measured under Broadway"). S1–S3 and S5 ran headless under GTK Broadway on 2026-09-30. S4, and live re-measures of S1, S5 and S6, ran in the user's Hyprland session later that day ([S4-live.md](spikes/S4-live.md)).

- **S1 Performance: mixed.** The 100 MB first screen passes (126 ms), and so does RAM, but only with highlighting off (2.67×; 7.04× with it). Typing fails right after a 10 MB Rust file opens (80–104 ms median) and at offset 0 (29.5 ms), but passes once highlighting settles (7.7 ms). Live on Hyprland, settled typing and offset 0 pass (2.7–5.4 ms median) and the post-open stall reproduces (70–77 ms median). The formatted long-line path passes and the raw insert freezes. Snapshots cost 27.7 ms at 50 MB. **Approved follow-up:** the syntax-highlighting size policy ([ADR-014](DECISIONS.md#adr-014--syntax-highlighting-size-policy-2026-09-30)). [S1-performance.md](spikes/S1-performance.md)
- **S2 Search: partial fail.** `replace_all` takes 8.0 s for 1M replacements against ≤ 5 s, and its undo takes 121 s with the view attached. It is one undo step, and `\U\1\E` works, but `\U$1` does not. **Approved follow-up:** our own replace engine and pattern translation ([ADR-005 amendment](DECISIONS.md#adr-005-amendment--our-own-replace-engine-2026-09-30)), bulk edits by edit count ([ADR-003 amendment](DECISIONS.md#adr-003-amendment--bulk-edits-by-edit-count-2026-09-30)), and a draft upstream issue for the binding bug ([upstream/sourceview5-replace-all.md](upstream/sourceview5-replace-all.md)). [S2-search.md](spikes/S2-search.md)
- **S3 Theme: pass.** 22/22 built-in schemes load, with 0 CSS errors. Live switching was correct across 28 themes, 5 of them light. The reload trigger fires once per swap and needs an extra `CREATED current` rule. **Approved follow-up:** that rule, covering any appearance of `current` ([ADR-004 amendment](DECISIONS.md#adr-004-amendment--reload-trigger-for-a-missing-current-directory-2026-09-30)). [S3-theme.md](spikes/S3-theme.md)
- **S4 Wayland/Hyprland: mostly pass.** A second launch focuses the running window, file dialogs float, and no planned shortcut collides with a Hyprland bind. fcitx5 grabs both UPPERCASE keys. Real-keyboard latency is 2.74 ms median and 14.4 ms p95, and SUPER+C copies. Not completed: the F-keys (an open question), drag-and-drop, SUPER+V, fcitx5 in a column caret, compose and fractional scaling. [S4-live.md](spikes/S4-live.md)
- **S5 Column mode: feasible.** A column insert over 10k lines takes 29 ms and is one undo step, and typing, Backspace and paste replicate. The fixes for tab stops, per-key undo steps and a slow attached undo are measured. fcitx5 still has to be checked by hand. **Approved follow-up:** exact tab stops in M1 ([ADR-015](DECISIONS.md#adr-015--exact-tab-stops-in-pango-units-2026-09-30)), grouped column-typing undo ([ADR-016](DECISIONS.md#adr-016--column-mode-editing-and-undo-model-2026-09-30)) and the bulk-edit rule for large rectangles. [S5-column.md](spikes/S5-column.md)
- **S6 Startup: startup passes on a warm cache.** Under Broadway it takes 56 ms from `main` and 109 ms from `exec` to the first frame, and second-launch forwarding takes 61 ms. Live on Hyprland it takes 224.5 ms from `main` and 273.6 ms from spawn, and 346.3 ms with a 10 MB file ([S4-live.md](spikes/S4-live.md)). `--wait` was not tested and waits for M2. [S1-performance.md, section e](spikes/S1-performance.md#e-s6-startup-5-runs-each-release-build)

The user approved these follow-ups on 2026-09-30; the spikes' other recommendations await review ([PLAN.md entry](PLAN.md#name-repository-and-spike-changes-approved--2026-09-30)). The reports ran under the working name Notes, and their raw output under `.local/` and `target/` was deleted on 2026-09-30 to free disk space; the reports keep the numbers.

The pass bars are in [PLAN.md](PLAN.md#milestone-plan) and [TESTING.md](TESTING.md#benchmarks-and-performance-targets). The consolidated summary is in the [PLAN.md entry of 2026-09-30](PLAN.md#m0-foundation-and-headless-spikes--2026-09-30), and what remains for M0 is in the [later entry of the same day](PLAN.md#name-repository-and-spike-changes-approved--2026-09-30).

---

## Sources

**Community and articles:** [MUO large files](https://www.makeuseof.com/best-apps-open-large-files-windows/), [SO survey 2025](https://survey.stackoverflow.co/2025/technology), Yahoo Tech on switching to a Linux text editor, It's FOSS on Linux editor alternatives.

**Omarchy:** local files under `/usr/share/omarchy/` (`bin/omarchy-theme-set`, `omarchy-theme-set-templates`, `omarchy-theme-color`, `omarchy-theme-refresh`, `omarchy-theme-set-gnome`, `omarchy-font-set`, `omarchy-launch-editor`, `omarchy-default-editor`, `omarchy-launch-or-focus`; `default/hypr/{helpers,looknfeel,windows}.lua`, `default/hypr/apps/{system,terminals}.lua`, `default/hypr/bindings/*.lua`; `default/themed/helix.toml.tpl`; `default/omarchy/omarchy-menu.jsonc`; `config/fcitx5/conf/clipboard.conf`), `/etc/fonts/conf.d/50-omarchy.conf`, `/usr/share/applications/{mimeapps.list,omawrite.desktop}`, `/usr/lib/fcitx5/libunicode.so`; upstream [omacom/omarchy](https://github.com/omacom/omarchy) (branch `quattro`, `manual/`), [theme manual](https://github.com/omacom/omarchy/blob/quattro/manual/43-making-your-own-theme.md); issues [#4842](https://github.com/basecamp/omarchy/issues/4842), [#7810](https://github.com/omacom/omarchy/issues/7810), [#13037](https://github.com/omacom/omarchy/issues/13037), [#12451](https://github.com/omacom/omarchy/pull/12451), [#2756](https://github.com/omacom/omarchy/issues/2756); discussions [#13697](https://github.com/omacom/omarchy/discussions/13697), [#8360](https://github.com/omacom/omarchy/discussions/8360), [#13399](https://github.com/omacom/omarchy/discussions/13399); [Omawrite](https://github.com/omacom-io/omawrite), [omacalc](https://github.com/omacom-io/omacalc); [Fcitx Unicode](https://fcitx-im.org/wiki/Unicode); [CachyOS Ctrl+Alt+F1 thread](https://discuss.cachyos.org/t/prevent-ctrl-alt-f1-from-freezing-screen-disable-vt-switch-completely/10173).

**Toolkits:** GtkSourceView [NEWS](https://gitlab.gnome.org/GNOME/gtksourceview/-/raw/master/NEWS), API: [View](https://gnome.pages.gitlab.gnome.org/gtksourceview/gtksourceview5/class.View.html), [FileLoader](https://gnome.pages.gitlab.gnome.org/gtksourceview/gtksourceview5/class.FileLoader.html), [SearchSettings](https://gnome.pages.gitlab.gnome.org/gtksourceview/gtksourceview5/class.SearchSettings.html), [Map](https://gnome.pages.gitlab.gnome.org/gtksourceview/gtksourceview5/class.Map.html), [StyleSchemeManager](https://gnome.pages.gitlab.gnome.org/gtksourceview/gtksourceview5/class.StyleSchemeManager.html), [LanguageManager](https://gnome.pages.gitlab.gnome.org/gtksourceview/gtksourceview5/class.LanguageManager.html); [sourceview5 features](https://docs.rs/crate/sourceview5/latest/features), [libadwaita features](https://docs.rs/crate/libadwaita/latest/features), [gtk4 TextViewImpl](https://docs.rs/gtk4/latest/gtk4/subclass/text_view/trait.TextViewImpl.html), [AdwShortcutsDialog](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/main/class.ShortcutsDialog.html); [Hergert, "Mid-life transitions"](https://blogs.gnome.org/chergert/2026/02/06/mid-life-transitions/); GNOME issues [Text Editor #253](https://gitlab.gnome.org/GNOME/gnome-text-editor/-/work_items/253), [#391](https://gitlab.gnome.org/GNOME/gnome-text-editor/-/work_items/391), [#645](https://gitlab.gnome.org/GNOME/gnome-text-editor/-/work_items/645), [GTK #229](https://gitlab.gnome.org/GNOME/gtk/-/issues/229), [GtkSourceView pain points](https://wiki.gnome.org/Projects/GtkSourceView/PainPoints), [GTK fractional scaling](https://blogs.gnome.org/gtk/2024/03/07/on-fractional-scales-fonts-and-hinting/), [gtk_disable_portals](https://docs.gtk.org/gtk4/func.disable_portals.html), [GApplicationCommandLine](https://docs.gtk.org/gio/class.ApplicationCommandLine.html), [GFileMonitorEvent](https://docs.gtk.org/gio/enum.FileMonitorEvent.html), [gtksourceview.c](https://raw.githubusercontent.com/GNOME/gtksourceview/master/gtksourceview/gtksourceview.c), [gtktextbuffer.c](https://raw.githubusercontent.com/GNOME/gtk/main/gtk/gtktextbuffer.c), [gtktexthistory.c](https://raw.githubusercontent.com/GNOME/gtk/main/gtk/gtktexthistory.c); iced [0.14.0](https://github.com/iced-rs/iced/releases/tag/0.14.0), [#2477](https://github.com/iced-rs/iced/issues/2477); [cosmic-text](https://github.com/pop-os/cosmic-text), [cosmic-edit](https://github.com/pop-os/cosmic-edit); [GPUI](https://github.com/zed-industries/zed/tree/main/crates/gpui), [gpui-kit](https://github.com/longbridge/gpui-kit), [gpui-kit editor](https://gpui-kit.com/component/editor/), [runner #733](https://github.com/yicheng47/runner/issues/733), Zed [#33464](https://github.com/zed-industries/zed/issues/33464), [#25195](https://github.com/zed-industries/zed/issues/25195), [#37918](https://github.com/zed-industries/zed/issues/37918); [CXX-Qt changelog](https://github.com/KDAB/cxx-qt/blob/main/CHANGELOG.md), [Qt Bridge for Rust 0.3](https://www.qt.io/blog/qt-bridge-for-rust-0.3-cxx-qt-compatibility-and-soundness); [winit #1881](https://github.com/rust-windowing/winit/issues/1881), [egui #1563](https://github.com/emilk/egui/issues/1563); [Slint #10859](https://github.com/slint-ui/slint/discussions/10859), [Xilem #388](https://github.com/linebender/xilem/issues/388), [Makepad PR #809](https://github.com/makepad/makepad/pull/809), [Floem](https://github.com/lapce/floem).

**Crates:** versions and licenses from the crates.io API on 2026-09-30; behaviour from upstream source or docs.rs, including [chardetng EncodingDetector](https://docs.rs/chardetng/latest/chardetng/struct.EncodingDetector.html), [pcre2 Regex](https://docs.rs/pcre2/latest/pcre2/bytes/struct.Regex.html), serde_json [#505](https://github.com/serde-rs/json/issues/505), [#721](https://github.com/serde-rs/json/issues/721), [#559](https://github.com/serde-rs/json/issues/559).

**Prior art:** [OmNote](https://github.com/litescript/omnote), [omarchy-notepad](https://github.com/design-nexus/omarchy-notepad), [OmaText](https://github.com/komagata/OmaText), [omarchy-working-memory](https://github.com/dogacid/omarchy-working-memory), [omarchy-scratchpad](https://github.com/RobbieUK1/omarchy-scratchpad); [Zed settings](https://zed.dev/docs/reference/all-settings), [Zed #64232](https://github.com/zed-industries/zed/issues/64232), [#27283](https://github.com/zed-industries/zed/issues/27283); [Lapce](https://github.com/lapce/lapce); gedit [49.0](https://gedit-text-editor.org/blog/2026-01-16-gedit-49-0.html), [50.0](https://gedit-text-editor.org/blog/2026-03-28-gedit-50-0.html); [GNOME Text Editor](https://github.com/GNOME/gnome-text-editor), [drafts help](https://help.gnome.org/gnome-text-editor/basics-draft-folder.html); [Kate](https://apps.kde.org/kate/), [KDE bug 353654](https://bugs.kde.org/show_bug.cgi?id=353654); [FeatherPad](https://github.com/tsujan/FeatherPad), [Geany](https://github.com/geany/geany), [Xed](https://github.com/linuxmint/xed); [Ferrite](https://github.com/OlaProeis/Ferrite), [FerrisPad](https://github.com/fedro86/ferrispad), [microsoft/edit](https://github.com/microsoft/edit), [fresh](https://github.com/sinelaw/fresh); [Sublime Text](https://www.sublimetext.com/download).
