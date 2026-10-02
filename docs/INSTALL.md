# Installing Stet

Project: Stet, a fast, keyboard-first text and code editor for Omarchy

Synced: 2026-10-02 (1.3.4)

Stet comes from the GitHub repository [PixDevsApps/Stet](https://github.com/PixDevsApps/Stet): as the Arch package attached to a release, or built from source. There is no AUR package yet. These steps are for Arch Linux and Omarchy; other distributions need the same libraries. Building details are in [BUILD.md](BUILD.md), using Stet in [USER_GUIDE.md](USER_GUIDE.md).

---

## What it needs

Runtime: `gtk4` (4.18 or newer), `libadwaita` (1.9), `gtksourceview5` (5.18 or newer), `glib2`, `pcre2` and `fontconfig`. On Omarchy they are installed already, as are `xdg-utils` (`xdg-open`, `xdg-mime`) and `xdg-terminal-exec`, which the tab menu's Open Containing Folder and Open Terminal Here use.

Building: Rust 1.92 or newer, `cargo` and `pkgconf`:

```sh
sudo pacman -S --needed rust gtk4 libadwaita gtksourceview5 pcre2 pkgconf
```

If Rust comes from rustup instead (`rustup default stable`), leave `rust` out of that line.

## Install the released package

The latest release, [v1.3.4](https://github.com/PixDevsApps/Stet/releases/tag/v1.3.4), has the Arch package attached; its SHA-256 is in the release notes. In a terminal (in Omarchy, **Super + Enter**):

```sh
curl -fLO https://github.com/PixDevsApps/Stet/releases/download/v1.3.4/stet-1.3.4-1-x86_64.pkg.tar.zst
sha256sum stet-1.3.4-1-x86_64.pkg.tar.zst    # optional: compare with the release notes
sudo pacman -U stet-1.3.4-1-x86_64.pkg.tar.zst
```

`sudo` asks for your password (nothing shows while you type it), and pacman asks *Proceed with installation? [Y/n]*: press Enter. The package is also on the release page for a download in the browser, and `gh release download v1.3.4 --repo PixDevsApps/Stet --pattern 'stet-*.pkg.tar.zst'` fetches it with the GitHub CLI. Stet then starts from the app launcher (in Omarchy, **Super + Space**) or with `stet` in a terminal. pacman installs the dependencies from the Arch repositories. The package upgrades an installed Stet and leaves your sessions and drafts in `~/.local/state/stet` alone; quit Stet (Ctrl+Alt+Q) and start it again to run the new version.

## Get the source

```sh
git clone https://github.com/PixDevsApps/Stet.git stet
cd stet
git checkout v1.3.4    # a release tag, or stay on main
```

The repository is public, so cloning needs no GitHub account.

## Run it from the source tree

```sh
cargo build
cargo run -- README.md
```

A debug build has its own application id, `io.github.pixdevsapps.Stet.Devel`, so it never forwards its files to an installed Stet and keeps its own single instance.

## Build and install the Arch package

The package is built from the **committed** tree:

```sh
python3 tools/package-source.py
cd .local/package
makepkg --cleanbuild --force
sudo pacman -U stet-<version>-1-x86_64.pkg.tar.zst
```

makepkg checks the build dependencies against pacman's database. With Rust from rustup that check fails for `rust` and `cargo`, which pacman cannot see; once the other dependencies are installed through pacman, build with `makepkg --cleanbuild --force --nodeps`.

The package installs `/usr/bin/stet`, the desktop entry `io.github.pixdevsapps.Stet.desktop` with Stet's text MIME types, the icon (hicolor, scalable, symbolic, 16 to 256 px), the license bundle in `/usr/share/licenses/stet/`, and this guide, the user guide, the README and INTEGRATIONS.md in `/usr/share/doc/stet/`. Check it with:

```sh
stet --version
pacman -Qkk stet
gtk-launch io.github.pixdevsapps.Stet
```

Stet then appears in the application launcher; on Hyprland its window class is `io.github.pixdevsapps.Stet`.

## Install for your user only (no sudo)

```sh
cargo build --release
install -Dm755 target/release/stet ~/.local/bin/stet
install -Dm644 packaging/io.github.pixdevsapps.Stet.desktop \
  ~/.local/share/applications/io.github.pixdevsapps.Stet.desktop
install -Dm644 assets/brand/io.github.pixdevsapps.Stet.svg \
  ~/.local/share/icons/hicolor/scalable/apps/io.github.pixdevsapps.Stet.svg
install -Dm644 assets/brand/io.github.pixdevsapps.Stet-symbolic.svg \
  ~/.local/share/icons/hicolor/symbolic/apps/io.github.pixdevsapps.Stet-symbolic.svg
update-desktop-database ~/.local/share/applications
gtk4-update-icon-cache -f -t ~/.local/share/icons/hicolor
```

`~/.local/bin` must be on `PATH` (it is on Omarchy) so that the desktop entry's `Exec=stet` and Omarchy's `omarchy-launch-editor` find Stet.

## Make Stet the default editor

Run **Settings › Set as Default Editor…** (or type "default editor" in the palette, Ctrl+Shift+P). It shows what it will change and asks first:

- it writes `stet` to `~/.local/state/omarchy/defaults/editor`, so SUPER+SHIFT+N (`omarchy-launch-editor`) opens Stet;
- it runs `xdg-mime default io.github.pixdevsapps.Stet.desktop` for the MIME types of Stet's desktop entry (`text/plain`, `text/markdown`, `application/json`, …), so double-clicking such a file in the file manager opens Stet. That writes your `~/.config/mimeapps.list`.

Afterwards it reports what changed. To undo it: `omarchy-default-editor nvim` (or delete `~/.local/state/omarchy/defaults/editor`), and `xdg-mime default nvim.desktop text/plain` for each type, or remove Stet's lines from `~/.config/mimeapps.list`. Details: [INTEGRATIONS.md](INTEGRATIONS.md#4-set-as-default-editor-built-in-m5).

## D-Bus activation and `--wait`

Stet runs as a single instance through GApplication: a second `stet file` opens the file in the running window. D-Bus activation (`/usr/share/dbus-1/services/io.github.pixdevsapps.Stet.service` and `DBusActivatable=true` in the desktop entry) and `stet --wait` for `GIT_EDITOR` and `SUDO_EDITOR` come with milestone M2 ([ADR-011](DECISIONS.md#adr-011--d-bus-activation-for-single-instance-and---wait-2026-09-30)); until then `--wait` is not accepted.

## Uninstall

```sh
sudo pacman -R stet
```

or, for a user-local install, delete `~/.local/bin/stet`, the desktop entry and the two icons listed above, then run `update-desktop-database ~/.local/share/applications`.

Your own files stay, and are yours to delete: the settings in `~/.config/stet/` (`config.toml`, `keys.toml`), the recent files (and, from M2, the session and backups of unsaved work) in `~/.local/state/stet/`, and the generated colour scheme in `~/.cache/stet/`. If you made Stet the default editor, undo that first (above).
