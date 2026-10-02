# Integrations

Project: Stet, a fast, keyboard-first text and code editor for Omarchy

Synced: 2026-10-02

The repository docs are the canonical project memory ([ADR-012](DECISIONS.md)). Dated entries describe the state at that date; newer entries take precedence.

---

## Status

**Partly implemented (M1, 2026-10-01):** the live theme (section 1), the font (section 2) and the desktop entry and icons (section 3, without D-Bus activation) are built and checked headless; the live-session checks are listed in [PLAN.md](PLAN.md). M3 (2026-10-01) adds the read-only banner's `sudoedit` command (section 5) and the handling of files changed by other programs (section 6a). M2 (2026-10-01) adds the D-Bus service file and `DBusActivatable=true` (section 3), and `--wait`, so `GIT_EDITOR` and `SUDO_EDITOR` work (section 5); its live checks are in [PLAN.md](PLAN.md#m2-built-and-checked-headless-live-checks-pending--2026-10-01). M5 (2026-10-01) adds "Set as default editor" (section 4), the UPPERCASE key and `keys.toml` around fcitx5 (section 6), the tab menu's terminal, file manager and trash (section 7) and the settings files (section 7a). The per-theme `stet.toml` (section 1) and the snippets of section 5 remain planned.

**Earlier status (2026-09-30):** draft; nothing was implemented yet unless a section says otherwise. The Omarchy facts were checked read-only on the development machine on 2026-09-30 (Omarchy 4.0.4, `omarchy` and `omarchy-settings` packages 4.0.4-1). `/usr/share/omarchy/version` still says `4.0.0.alpha`; the pacman package is the source of truth. The command `stet` and the app-id `io.github.pixdevsapps.Stet` below are final since 2026-09-30 ([ADR-010](DECISIONS.md#adr-010-accepted--stet-2026-09-30)). This page is completed in M5 and installed to `/usr/share/doc/`.

## Principles

- Stet must run without Omarchy. Missing Omarchy files mean fallbacks, not errors.
- The package never edits `~/.config`, `~/.local/state/omarchy` or files owned by the `omarchy` packages. It installs only its own files under `/usr`.
- Anything that changes the user's setup is an opt-in command that asks for confirmation, or a snippet the user applies by hand.
- Hyprland rules can't be added cleanly by a package: Omarchy loads only `$OMARCHY_PATH/default/hypr/apps/*.lua`, which the `omarchy` package owns. Rules are documented as snippets instead.

## 1. Theme (ADR-004; built in M1)

As built on 2026-10-01 ([ADR-004 amendment](DECISIONS.md#adr-004-amendment--as-built-in-m1-2026-10-01)): the parent `~/.local/state/omarchy/` is always watched as well, so `current` appearing or disappearing reloads at once; `omarchy-theme-color` and the scheme write run off the GTK thread; `STET_OMARCHY_STATE_DIR` redirects the whole directory for tests. The per-theme `stet.toml` override below is not built yet.

### Paths

```
~/.local/state/omarchy/current/        <- watched, non-recursive
  theme/            real directory, replaced on every theme change
    colors.toml     the palette Stet reads
    stet.toml       optional colour-only override (a theme or a user template may provide it)
  next-theme/       staging directory used during a theme change (ignored)
  theme.name        rewritten right after the swap
  background        symlink
```

### How a theme change happens

`/usr/share/omarchy/bin/omarchy-theme-set` (serialized with `flock` on `$XDG_RUNTIME_DIR/omarchy-theme-set.lock`):

1. Builds `current/next-theme` from the built-in theme plus the user's theme, then renders templates into it.
2. Swaps it in: `rm -rf current/theme` then `mv current/next-theme current/theme` (lines 292–293).
3. Writes the theme name: `echo "$THEME_NAME" > current/theme.name` (line 296).
4. Notifies the shell, restarts or reconfigures other apps, and runs `omarchy-hook theme-set`.

`omarchy-theme-refresh` re-runs `omarchy-theme-set` with the current name (skipping the background), so it goes through the same swap.

### Reload mechanics

- GFileMonitor on the directory `~/.local/state/omarchy/current/`, with `WATCH_MOVES`.
- Trigger on `theme.name` CHANGES_DONE_HINT, or on RENAMED where the new name is `theme`. With `WATCH_MOVES`, `mv next-theme theme` inside the watched directory arrives as RENAMED, not as MOVED_IN or CREATED. Ignore events on `next-theme`.
- If `current/` doesn't exist yet when the watch starts, watch the parent `~/.local/state/omarchy/` and also trigger when `current` appears there (CREATED, MOVED_IN, or RENAMED to `current`); then watch `current/` itself ([ADR-004 amendment](DECISIONS.md#adr-004-amendment--reload-trigger-for-a-missing-current-directory-2026-09-30), from S3, where a watch started before `current/` existed missed the first swap).
- Debounce 150 ms, then resolve the palette again and apply it ([DESIGN.md](DESIGN.md#theme-tokens)).
- If the monitor can't be created, poll every 2 s. If `current/` doesn't exist, use the fallback palettes until it appears.
- No `theme-set.d` hook is needed: hooks run after the swap, which the monitor already sees.

### Palette resolution

When `/usr/share/omarchy/bin/omarchy-theme-color` exists, Stet runs:

```sh
omarchy-theme-color --file ~/.local/state/omarchy/current/theme/colors.toml --all
```

It prints every resolved key and value, tab-separated and sorted by key (for example `accent	#a86660`), with every alias and fallback applied: the same cascade the Omarchy templates use. It took about 8 ms on this machine (technical critique). A Rust port of the cascade is used only when the script is missing. Stet supplies its own `accent` fallback, because the script has none.

Resolved keys: `mode`, `accent`, `selection`, `selection_background`, `selection_foreground`, `muted`, `cursor`, `background`, `dark_background`, `darker_background`, `lighter_background`, `foreground`, `dark_foreground`, `light_foreground`, `bright_foreground`, `red`, `yellow`, `orange`, `green`, `cyan`, `blue`, `magenta`, `brown`, `bright_*`, plus the aliases `purple`, `color0`–`color15` and the legacy short names. Mode precedence: the `mode` key, then `theme_type`, then a `light.mode` file next to `colors.toml`, then background luminance, then dark. All 22 built-in themes set `mode` (17 dark, 5 light).

### Per-theme override (planned)

If `current/theme/stet.toml` exists, its colour-only roles win over the generated mapping. Omarchy's template engine never overwrites a file a theme ships itself. Users who want an override for every theme can copy the shipped `stet.toml.tpl.sample` (from `/usr/share/doc/`) to `~/.config/omarchy/themed/stet.toml.tpl`; it renders on the next `omarchy theme set` or `omarchy-theme-refresh`. Keep it colour data only: no paths, code or `.lua`.

## 2. Font (built in M1)

As built on 2026-10-01: `fc-match` runs off the GTK thread; the family goes into CSS for the editor (11 pt plus zoom) and the monospace chrome; `~/.config/fontconfig/` is watched, and its parent until it exists, because `omarchy-font-set` creates the directory and writes `fonts.conf` in one go.

- fontconfig is Omarchy's source of truth. `omarchy-font-set <name>` writes `~/.config/fontconfig/fonts.conf` with a `monospace` rule; the system default in `/etc/fonts/conf.d/50-omarchy.conf` maps `monospace` to JetBrainsMono Nerd Font.
- Stet resolves the family with `fc-match monospace -f '%{family[0]}'` (today: `JetBrainsMono Nerd Font`) and applies it explicitly through CSS.
- It watches `~/.config/fontconfig/` (which may not exist yet; GLib polls missing paths) and re-resolves on changes. Whether GTK/Pango notices fontconfig changes by itself in a running process is unverified, so the explicit re-resolve is the plan.
- Don't read gsettings `monospace-font-name`: Omarchy doesn't update it (it still says `Adwaita Mono 11`).
- Size 11 pt by default; zoom is per session.
- Tab stops are set in Pango units from the resolved font's exact advance, and re-applied when the font, the zoom or the tab width changes ([ADR-015](DECISIONS.md#adr-015--exact-tab-stops-in-pango-units-2026-09-30)).

## 3. Desktop entry, D-Bus service and MIME (entry built in M1; D-Bus in M2)

**M1, 2026-10-01:** `packaging/io.github.pixdevsapps.Stet.desktop` is the draft below **without** `DBusActivatable=true`, which comes with the service file in M2. `desktop-file-validate` accepts it with one hint: `Utility;TextEditor;Development;` has two main categories (Utility and Development), so a menu may list Stet twice. The categories were kept as planned. The icon is installed as `io.github.pixdevsapps.Stet` in hicolor (scalable, symbolic, 16–256 px; [ASSETS.md](ASSETS.md)).

**M2, 2026-10-01:** the desktop file now sets `DBusActivatable=true` (`desktop-file-validate` still gives only the categories hint), and `packaging/io.github.pixdevsapps.Stet.service` is the service file below; the PKGBUILD installs it in `/usr/share/dbus-1/services/` ([ADR-011 amendment](DECISIONS.md#adr-011-amendment--d-bus-activation-and---wait-as-built-in-m2-2026-10-01)). A launcher or file manager that honours `DBusActivatable` asks the bus for `io.github.pixdevsapps.Stet`, which starts `stet --gapplication-service` if it isn't running; a command line (`stet file`, or a launcher that runs `Exec=` itself) asks the bus to start it too and then hands over its files. On Omarchy the session bus is dbus-broker, which runs an activated service as a transient systemd user unit (`dbus-:1.<n>-io.github.pixdevsapps.Stet@<n>.service`) with the user manager's environment. dbus-broker reads service files when it starts or reloads: after installing one by hand, run `busctl --user call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus ReloadConfig`. A user-local install puts it in `~/.local/share/dbus-1/services/` with the absolute path of its `stet` in `Exec=`.

**1.3.3, 2026-10-02:** the comment is "Edit text and code files, keyboard-first", at the user's request.

`/usr/share/applications/io.github.pixdevsapps.Stet.desktop`, drafted 2026-09-30, with the comment of 1.3.3:

```ini
[Desktop Entry]
Type=Application
Name=Stet
GenericName=Text Editor
Comment=Edit text and code files, keyboard-first
Exec=stet %F
Icon=io.github.pixdevsapps.Stet
Terminal=false
StartupNotify=true
StartupWMClass=io.github.pixdevsapps.Stet
DBusActivatable=true
Categories=Utility;TextEditor;Development;
Keywords=text;editor;code;notepad;
MimeType=text/plain;text/markdown;text/rust;application/toml;text/x-shellscript;application/json;application/xml;application/yaml;text/x-python;text/x-csrc;text/x-c++src;text/x-makefile;text/x-log;text/css;text/html;text/javascript;
```

- The file name must equal the app-id, because `DBusActivatable=true` looks the application up by its D-Bus name, and Hyprland's class for GTK4 apps is the app-id.
- Every MIME name above exists in shared-mime-info 2.5.1 on this machine. `text/x-rust` and `text/x-toml` do not exist; `application/x-shellscript`, `application/javascript` and `application/x-yaml` are only aliases.
- `Actions=new-window` is added only once a `--new-window` flag exists.
- Icons go to hicolor (scalable plus PNG sizes), as in Hertz.
- **The app-id must never match `org.omarchy.*` or `TUI.*`.** Omarchy tags those classes as `terminal` (`default/hypr/apps/terminals.lua`), and `SUPER+C/V` then send Ctrl+Insert / Shift+Insert instead of Ctrl+C / Ctrl+V.

Draft `/usr/share/dbus-1/services/io.github.pixdevsapps.Stet.service` ([ADR-011](DECISIONS.md)):

```ini
[D-BUS Service]
Name=io.github.pixdevsapps.Stet
Exec=/usr/bin/stet --gapplication-service
```

Omarchy's own MIME defaults live in `/usr/share/applications/mimeapps.list` (owned by `omarchy-settings`; `text/plain=nvim.desktop`). Stet never modifies that file.

## 4. "Set as default editor" (built in M5)

An opt-in command: Settings › Set as Default Editor… and the palette. It shows what it will change and asks for confirmation, then:

1. writes `stet` to `~/.local/state/omarchy/defaults/editor`, so `SUPER+SHIFT+N` (`omarchy-launch-editor`) starts Stet;
2. runs `xdg-mime default io.github.pixdevsapps.Stet.desktop text/plain text/markdown …` for the MIME types in the desktop file, which writes the user's `~/.config/mimeapps.list`, so double-clicking in Nautilus opens Stet (the gap in Omarchy #7810).

As built on 2026-10-01 (`app/src/window/default_editor.rs`, `infrastructure::omarchy::defaults`, `infrastructure::desktop`):

- **Before asking**, a worker reads the current default-editor command, asks `xdg-mime query default text/plain`, looks for `stet` on `PATH` and for the desktop entry in `$XDG_DATA_HOME/applications` and every `$XDG_DATA_DIRS/applications`. The confirmation names the file and its current command, the number of MIME types (16, from the desktop entry, compiled in from `packaging/io.github.pixdevsapps.Stet.desktop`) and what opens `text/plain` now, warns when `stet` is not on `PATH` (`omarchy-launch-editor` would fall back to nvim) or the desktop entry is not installed (a double-click could not open Stet), and says how to undo it.
- **Writing**: the command goes through a temporary file renamed into place, `stet` and a line break as `omarchy-default-editor` writes it; then one `xdg-mime default <id> <types…>`. The file is in Omarchy's state directory as Stet resolves it for the theme: `$HOME/.local/state/omarchy`, where `omarchy-launch-editor` reads it, whatever `XDG_STATE_HOME` says (since 2026-10-01; before, Stet followed `XDG_STATE_HOME`).
- **The report** (a dialog, then a toast) says what the file held before and holds now, and what `xdg-mime query default text/plain` reports before and after. `xdg-mime` reports a desktop entry only when its `Exec` program exists, so with Stet installed it reports `io.github.pixdevsapps.Stet.desktop`.
- **Tested in a sandbox only** (`default-editor.stet-test`): the self-test points `HOME` and the XDG directories at its scratch directory, and Stet at a scratch Omarchy directory, so the user's `mimeapps.list` and default-editor file are never touched.

Background:

- `omarchy-launch-editor` reads the state file and falls back to `nvim` when the command is missing. Terminal editors (`nvim`, `vim`, `nano`, `micro`, `hx`, `helix`, `fresh`) run in a terminal; **anything else runs detached** as `exec setsid uwsm-app -- "$editor" "$@"`, and `--inline` is ignored for it.
- `omarchy-default-editor` only accepts `code|cursor|zed|sublime_text|helix|vim|emacs|nvim`, so Stet writes the state file directly. Proposing Stet for that list upstream comes after 1.0.
- `$EDITOR` is `omarchy-launch-editor --inline`. Because GUI editors are started detached, `$EDITOR` never waits for Stet; use `GIT_EDITOR` and `SUDO_EDITOR` with `--wait` (section 5).
- To undo: `omarchy-default-editor nvim` (or delete the state file), and `xdg-mime default nvim.desktop text/plain` for each type, or remove the lines from `~/.config/mimeapps.list`.

This command is the only place where Stet changes user configuration, and it never runs on its own.

## 5. Documented snippets (planned)

All of these are applied by the user. The syntax was checked against the installed Omarchy 4.0.4 files; the `stet` command and its app-id are not installed yet (there is no package before M1).

### Hyprland key binding

In `~/.config/hypr/bindings.lua` (the user's binding file, loaded after Omarchy's defaults). `SUPER+SHIFT+T` is free in Omarchy 4.0.4's default bindings and in this machine's user bindings:

```lua
o.bind("SUPER + SHIFT + T", "Stet", { launch = "stet", focus = "^io\\.github\\.pixdevsapps\\.Stet$" })
```

`o.bind` comes from `default/hypr/helpers.lua`: `{ launch = …, focus = … }` becomes `omarchy-launch-or-focus '<pattern>' 'uwsm-app -- stet'`, which focuses a window whose class or title matches the pattern (case-insensitive) and launches Stet otherwise. It is the same form as Omarchy's own `o.bind("SUPER + SHIFT + O", "Obsidian", { launch = "obsidian", focus = "^obsidian$" })` in `default/hypr/bindings/applications.lua`. With a single-instance app, plain `{ launch = "stet" }` also works: a second launch just raises the existing window.

### Opacity

Omarchy tags every window `default-opacity` (`opacity = "0.985 0.96"`). To keep text fully opaque, add to `~/.config/hypr/hyprland.lua`, following the pattern in `default/hypr/apps/system.lua`:

```lua
o.window("io.github.pixdevsapps.Stet", { tag = "-default-opacity" })
o.window("io.github.pixdevsapps.Stet", { opacity = "1 1" })
```

The class is matched in full, so debug builds (`io.github.pixdevsapps.Stet.Devel`) are not affected. This machine's `looknfeel.lua` already makes every window opaque.

### Omarchy menu

In `~/.config/omarchy/extensions/omarchy-menu.jsonc` (user-owned, reloaded automatically). Dotted ids place rows in submenus; `when` hides a row when its shell condition fails; `checked` adds a check mark when it succeeds:

```jsonc
"personal": {"icon":"","label":"Personal"},
"personal.stet": {"icon":"󰎞","label":"Stet","action":"uwsm-app -- stet"},
"setup.default.editor.stet": {"icon":"󰎞","label":"Stet","when":"omarchy-cmd-present stet",
  "checked":"[[ \"$(omarchy-default-editor)\" == \"stet\" ]]",
  "action":"mkdir -p ~/.local/state/omarchy/defaults && printf 'stet\\n' > ~/.local/state/omarchy/defaults/editor"}
```

The `setup.default.editor.*` rows match the built-in ones in `/usr/share/omarchy/default/omarchy/omarchy-menu.jsonc`, so Stet appears under Setup → Defaults → Editor. That row changes only the state file, not the MIME defaults; the in-app "Set as default editor" command does both. The menu can't host a custom "recent files" provider in 4.0.4: the providers are hard-coded in the shell's `Menu.qml`.

### `GIT_EDITOR` and `SUDO_EDITOR`

In your shell's rc file (for example `~/.bashrc`):

```sh
export GIT_EDITOR="stet --wait"
export SUDO_EDITOR="stet --wait"
```

or for git only: `git config --global core.editor "stet --wait"`. Then `git commit` and `sudoedit /etc/hosts` open a tab in the running Stet (or start it through D-Bus activation) and wait until **that tab** closes, not the whole app. Closing a `--wait` tab without saving exits with status 1, so git aborts instead of committing a stale message. `--wait` files are never added to the session. `sudoedit` is the supported way to edit root-owned files; the read-only banner points to it.

As built in M3 (2026-10-01): a file the user may not write opens read-only, and its banner shows `SUDO_EDITOR="stet --wait" sudoedit <path>` (the path quoted for the shell) with a Copy Command button; a save that meets a permission error shows the same command. `--wait` itself arrives with M2, so until then the command stops at Stet's argument parser.

As built in M2 (2026-10-01): `stet --wait FILE...` opens the files as tabs in the running Stet, starting it through D-Bus activation when it isn't running, and returns when all of them are closed: status 0, or 1 when one was closed without saving its changes (Discard), did not open, or Stet quit while it had unsaved changes. Other tabs and the editor itself stay open. A file that is open already counts too. `--wait` files are kept out of the session and the recent files. A development build without an installed service file (app-id `io.github.pixdevsapps.Stet.Devel`) starts `stet --gapplication-service` in the background itself for a `--wait` command line, so `GIT_EDITOR="<absolute path>/target/debug/stet --wait"` works as well; that background Stet then lives in the terminal's process tree (and session scope), not in a unit of its own. Not verified yet: whether `sudoedit` keeps `DBUS_SESSION_BUS_ADDRESS` (or `XDG_RUNTIME_DIR`) for the editor; without a bus, `stet --wait` becomes the editor itself and waits until it quits.

## 5a. `hyprctl dispatch` on Hyprland 0.56 (verified 2026-09-30)

With Omarchy 4's Lua configuration, `hyprctl dispatch` takes a Lua expression. The older `hyprctl dispatch focuswindow address:…` form is rejected with a Lua syntax error, and nothing happens if the reply is ignored. Use the forms `omarchy-launch-or-focus` uses:

```sh
hyprctl dispatch 'hl.dsp.focus({ window = "address:0x…" })'
hyprctl dispatch 'hl.dsp.exec_cmd("stet README.md")'
hyprctl dispatch 'hl.dsp.window.close({ window = "address:0x…" })'
```

Verified live on 2026-09-30 ([S4-live.md](spikes/S4-live.md)):

- With Omarchy's `focus_on_activate = true`, a second launch from a shell or through `hl.dsp.exec_cmd` (the SUPER+SHIFT+N path) focuses the running window and opens the file as a tab. No window rule is needed.
- GtkFileDialog goes through `xdg-desktop-portal-gtk`, and Omarchy floats it (875×600). No window rule is needed.

## 6. fcitx5 key grabs

fcitx5 5.1.22 runs on this machine. `~/.config/fcitx5/conf/` overrides only the clipboard addon's hotkey (Omarchy's default); the trigger and Unicode hotkeys are at their defaults. It takes some keys before any app sees them:

- **Ctrl+Space** is fcitx5's default trigger key (Omarchy #4842). Stet puts completion on Ctrl+Enter instead.
- **Ctrl+Shift+U** and **Ctrl+Alt+Shift+U** are the Unicode addon's defaults (both found in `/usr/lib/fcitx5/libunicode.so`). They are UPPERCASE and Sentence case (blend) in the classic editor keymap, so Stet's UPPERCASE is **Alt+Shift+U** (M5; Proper Case (blend) in that keymap, which has no key in Stet) and Sentence case (blend) has no key ([ADR-009 amendment](DECISIONS.md#adr-009-amendment--text-tool-keys-m5-2026-10-01)).
- **Ctrl+Alt+F1–F12** switch virtual terminals. Stet moves the volatile find keys to Alt+F3 / Alt+Shift+F3.
- **Ctrl+Alt+P** is fcitx5's default "toggle embedded preedit" key (found in `libFcitx5Core.so` on 2026-10-01). Stet does not use it.

To give Ctrl+Shift+U back to applications, clear the Unicode addon's hotkeys in `fcitx5-configtool` (Addons → Unicode), or create `~/.config/fcitx5/conf/unicode.conf`:

```ini
TriggerKey=
DirectUnicodeMode=
```

then restart fcitx5 or log in again, and give UPPERCASE its classic key, Ctrl+Shift+U, in `~/.config/stet/keys.toml` (Settings › Open Keyboard Shortcuts):

```toml
uppercase = ["<Control><Shift>u", "<Alt><Shift>u"]
```

Omarchy clears the clipboard addon's hotkey the same way (`/usr/share/omarchy/config/fcitx5/conf/clipboard.conf` contains `TriggerKey=`). The `TriggerKey` option name was confirmed in the installed binary; `DirectUnicodeMode` is fcitx5's upstream name for the Ctrl+Shift+U option and was **not** confirmed in the binary, so check it in `fcitx5-configtool` if the file has no effect.

## 6a. Open files changed by other programs (built in M3, 2026-10-01)

- Each open file has a GFileMonitor with `WATCH_MOVES`, so writes in place, deletions and files renamed into place (`git checkout`, `sed -i`, editors that save through a temporary file) are all seen. Events are debounced for 300 ms, then the file is compared with the tab's baseline by one `stat` on a worker.
- File systems that send no change events for remote writes (NFS, SMB, sshfs and other FUSE mounts) are covered by a check of every open file when Stet's window gets focus, at most once a second. A file changed elsewhere while Stet is unfocused is noticed when you switch back.
- The device number is left out of the comparison: btrfs subvolumes and device-mapper volumes can get new device numbers after a reboot ([ADR-006 amendment](DECISIONS.md#adr-006-amendment--one-fingerprint-type-2026-10-01)).
- A tab without unsaved changes reloads quietly (one undo step, with a toast); otherwise a banner asks. Stet never switches tabs, takes focus or asks Hyprland for attention because a file changed.

## 7. Other desktop actions

Built in M5 (2026-10-01) unless the table says otherwise; the tab menu's commands act on the tab that was clicked, and the File menu's on the current tab.

| Action | Implementation |
| --- | --- |
| Open a terminal here (tab menu, File) | `setsid --fork uwsm-app -- xdg-terminal-exec --dir=<dir>`, the pattern `omarchy-launch-terminal` uses (`TERMINAL=xdg-terminal-exec`); without `uwsm-app` or `setsid` (outside Omarchy) the command is shortened; without `xdg-terminal-exec` a toast says so. `setsid --fork` hands the terminal off at once, so Stet waits for nothing and leaves no child behind |
| Open containing folder (tab menu, File) | `setsid --fork uwsm-app -- xdg-open <dir>`, shortened the same way: the default file manager opens the file's folder |
| Move to trash (tab menu, File) | after a confirmation, `gio::File::trash` (GLib's trash in `$XDG_DATA_HOME/Trash`, or the volume's `.Trash-$UID`), then the tab closes; GLib refuses to trash on system mounts such as `/tmp` and the error is shown |
| Rename a file (Rename… in the tab menu and the File menu, a double-click on the tab; on an untitled tab it only names the tab, since 1.2) | `renameat2` with `RENAME_NOREPLACE` in the same folder, so an existing file is never replaced; the tab, the recent files and the file monitor follow the new name, and the language is detected again |
| Copy full path, file name, directory path (tab menu, Edit › Copy to Clipboard) | GTK's clipboard |
| File dialogs | GtkFileDialog, which uses the portal by default since GTK 4.18; Omarchy floats `xdg-desktop-portal-gtk` windows (`default/hypr/apps/system.lua`) |
| Clipboard | GTK's clipboard. `SUPER+C/V/X` send Ctrl+C/V/X to non-terminal windows (`default/hypr/bindings/clipboard.lua`), so standard keys must keep working |
| Activation focus | Omarchy sets `focus_on_activate = true` (`default/hypr/looknfeel.lua`); S4 checks that a second launch focuses the window |
| Full screen | F11 or Alt+Enter in the app (M1); `SUPER+F` from Hyprland |

## 7a. Settings files (built in M5)

`$XDG_CONFIG_HOME/stet/config.toml` and `keys.toml` ([ADR-017](DECISIONS.md#adr-017--settings-in-configtoml-and-keystoml-2026-10-01)) are Stet's only configuration. The package never writes them; Settings › Open Settings and Open Keyboard Shortcuts create one with every default commented out when it does not exist, and open it in a tab. Stet watches the directory, and its parent so that it notices the directory appear, and applies a file when it changes, from Stet or from any other program. Omarchy's own files are never written, except by "Set as default editor".

## 8. Not planned

- Installing anything into `/usr/share/omarchy/` (for example a default template) or adding a theme-set hook.
- Modifying `/usr/share/applications/mimeapps.list`.
- Shipping Hyprland rules in the package.
- Always-on-top: Wayland's xdg-shell gives apps no way to request it; Omarchy's `SUPER+O` pops out any window.
- An omarchy-shell plugin before 1.x (a quick-note or recent-files panel would read a file Stet writes).
