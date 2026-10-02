#!/usr/bin/env python3
"""Live acceptance checks for Stet on the user's Hyprland session (docs/TESTING.md).

Runs the release build under its own app-id (so it never meets a real Stet instance) and
drives it with real input: Hyprland's send_key_state for keys (resolved in the real keyboard's
keymap, like Omarchy's universal clipboard binds), tools/live/pointer for the mouse, Hyprland's
Lua dispatchers for focus, and AT-SPI to read the editor text, the caret and dialogs. Windows open on the current workspace; don't type while
it runs. Focus and the clipboard text are restored afterwards.

Usage: python3 tools/live/acceptance.py [--build] [--only NAME[,NAME...]] [--list]
Needs: Hyprland 0.56+ (Lua dispatch), grim, wl-clipboard, PyGObject with Atspi,
and `cargo build --release` in tools/live/pointer.
"""

import argparse
import json
import os
import re
import shutil
import signal
import statistics
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi, GLib  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
EXE = ROOT / "target/release/stet"
POINTER = ROOT / "tools/live/pointer/target/release/stet-live-pointer"
DRAG_SOURCE = ROOT / "tools/live/drag_source.py"
APP_ID = "io.github.pixdevsapps.Stet.LiveTest"
PORTAL = "xdg-desktop-portal-gtk"
THEMES = Path("/usr/share/omarchy/themes")


# Stet's own state and cache directories for the running check (set by main()), so sessions
# and backups never touch the user's real ones or leak from one check into the next. Omarchy's
# state is found through $HOME, so the real theme still applies.
SANDBOX = {}


def stet_env(extra=None):
    env = dict(os.environ, STET_APP_ID=APP_ID, RUST_LOG="info")
    env.update(SANDBOX)
    env.update(extra or {})
    return env


class Failed(Exception):
    pass


class Skipped(Exception):
    pass


# ----------------------------------------------------------------------- Hyprland and input

def hypr(what):
    out = subprocess.run(["hyprctl", "-j", what], capture_output=True, text=True).stdout
    return json.loads(out or "null")


def lua(expression):
    reply = subprocess.run(["hyprctl", "dispatch", expression], capture_output=True, text=True)
    if reply.stdout.strip() != "ok":
        raise Failed(f"hyprctl dispatch {expression!r}: {reply.stdout.strip() or reply.stderr.strip()}")


def focus(address):
    lua(f'hl.dsp.focus({{ window = "address:{address}" }})')


def active():
    window = hypr("activewindow") or {}
    return window.get("class"), window.get("address")


def wait_for(predicate, timeout=5.0, interval=0.05, what="condition"):
    """Polls until `predicate` returns something truthy. An AT-SPI error counts as "not yet":
    GTK replaces accessibles while dialogs open and close, so a lookup can meet a vanished one."""
    deadline = time.monotonic() + timeout
    last_error = None
    while time.monotonic() < deadline:
        try:
            value = predicate()
        except GLib.Error as error:
            last_error, value = error, None
        if value:
            return value
        time.sleep(interval)
    detail = f" (last AT-SPI error: {last_error.message})" if last_error else ""
    raise Failed(f"timed out after {timeout:.1f} s waiting for {what}{detail}")


def children(node):
    """The node's children, skipping any that vanish while we look."""
    try:
        count = node.get_child_count()
    except GLib.Error:
        return
    for i in range(count):
        try:
            child = node.get_child_at_index(i)
        except GLib.Error:
            continue
        if child is not None:
            yield child


def alive(test, node):
    """`test(node)`, or False when the node has gone away."""
    try:
        return test(node)
    except GLib.Error:
        return False


def require_active(cls):
    if active()[0] != cls:
        raise Failed(f"refusing to send keys: the active window is {active()[0]!r}, not {cls!r}")


MODIFIERS = {"ctrl": "CTRL", "shift": "SHIFT", "alt": "ALT", "super": "SUPER"}
CHARACTER_KEYS = {" ": "space", ".": "period", ",": "comma", "-": "minus", "+": "plus",
                  "'": "apostrophe", "<": "less", "\n": "Return", "\t": "Tab"}
# Characters on the shifted level of the user's Norwegian layout: send_key_state only finds a
# key's first-level keysym ("colon" is "key not found"), so these are Shift with the key.
SHIFTED_KEYS = {":": "period", ";": "comma", "_": "minus", "/": "7", "!": "1", '"': "2",
                "#": "3", "%": "5", "&": "6", "(": "8", ")": "9", "=": "0", "?": "plus",
                "*": "apostrophe", ">": "less"}


def press(mods, key):
    """One key press through Hyprland's send_key_state. Hyprland resolves `key` in the keymap of
    the keyboard used last, so this behaves like the real keyboard. (wtype is not used: its
    per-key keymaps break GTK's Shift+letter accelerators, and after it has typed, Hyprland
    resolves keys in wtype's partial keymap.)"""
    for state in ("down", "up"):
        lua(f'hl.dsp.send_key_state({{ mods = "{mods}", key = "{key}", state = "{state}" }})')
        time.sleep(0.02)


def keys(combo, target=APP_ID):
    """Sends one chord such as "ctrl+shift+p" or "Escape" to `target`, which must have focus."""
    require_active(target)
    parts = combo.split("+")
    key = parts[-1]
    press(" ".join(MODIFIERS[p] for p in parts[:-1]), key.upper() if len(key) == 1 else key)
    time.sleep(0.15)


def type_text(text, target=APP_ID):
    """Types ASCII text key by key into whatever has focus in `target`."""
    require_active(target)
    for char in text:
        if char.isascii() and char.isalpha():
            press("SHIFT" if char.isupper() else "", char.upper())
        elif char.isdigit():
            press("", char)
        elif char in CHARACTER_KEYS:
            press("", CHARACTER_KEYS[char])
        elif char in SHIFTED_KEYS:
            press("SHIFT", SHIFTED_KEYS[char])
        else:
            raise Failed(f"type_text cannot type {char!r}; insert it through AT-SPI instead")
    time.sleep(0.15)


def universal_shortcut(key, target=APP_ID):
    """What Omarchy's SUPER+C/V/X binds do for a non-terminal window: Hyprland injects Ctrl+key."""
    require_active(target)
    press("CTRL", key)
    time.sleep(0.2)
    return "compositor"


def clipboard():
    reply = subprocess.run(["wl-paste", "--no-newline", "--type", "text/plain"],
                           capture_output=True, text=True)
    return reply.stdout if reply.returncode == 0 else None


def drag(x1, y1, x2, y2):
    monitor = next(m for m in hypr("monitors") if m["focused"])
    subprocess.run([str(POINTER), "--extent", str(monitor["width"]), str(monitor["height"]),
                    "drag", str(x1), str(y1), str(x2), str(y2), "40", "15"], check=True)


def pixel(x, y):
    """The RGB colour of one screen pixel, through grim's PPM output."""
    raw = subprocess.run(["grim", "-g", f"{x},{y} 1x1", "-t", "ppm", "-"],
                         capture_output=True, check=True).stdout
    return tuple(raw[-3:])


def near(a, b, tolerance=10):
    return all(abs(x - y) <= tolerance for x, y in zip(a, b))


def hex_rgb(value):
    value = value.lstrip("#")
    return tuple(int(value[i:i + 2], 16) for i in (0, 2, 4))


# ------------------------------------------------------------------------------------ Stet

class Stet:
    """One Stet process under the test app-id, with its log collected."""

    def __init__(self, *args, env=None, wait_window=True):
        self.lines = []
        self.proc = subprocess.Popen([str(EXE), *map(str, args)], env=stet_env(env),
                                     stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        self.pid = self.proc.pid
        threading.Thread(target=self._collect, daemon=True).start()
        if wait_window:
            self.window()

    @classmethod
    def attach(cls, pid):
        """A Stet this script did not start (a background service started by `--wait` or by
        D-Bus); it has no log."""
        stet = cls.__new__(cls)
        stet.lines, stet.proc, stet.pid = [], None, pid
        return stet

    def alive(self):
        if self.proc is not None:
            return self.proc.poll() is None
        try:
            os.kill(self.pid, 0)
            return True
        except ProcessLookupError:
            return False

    def _collect(self):
        for line in self.proc.stdout:
            self.lines.append(line.rstrip("\n"))

    def log_count(self, needle):
        return sum(needle in line for line in self.lines)

    def wait_log(self, needle, count=1, timeout=5.0):
        return wait_for(lambda: self.log_count(needle) >= count, timeout, what=f"log {needle!r} ×{count}")

    def client(self):
        return next((c for c in hypr("clients")
                     if c.get("class") == APP_ID and c.get("pid") == self.pid), None)

    def window(self, timeout=10.0):
        """Waits for the window, then focuses it explicitly: Hyprland may keep the focus where
        the user is working."""
        client = wait_for(self.client, timeout, what="the Stet window")
        if active()[0] != APP_ID:
            focus(client["address"])
            wait_for(lambda: active()[0] == APP_ID, what="focus on the new Stet window")
        return client

    def title(self):
        client = self.client()
        return client["title"] if client else ""

    def accessible(self, timeout=5.0):
        """Stet's accessibility root; a new process takes a moment to appear on the bus."""
        def lookup():
            desktop = Atspi.get_desktop(0)
            for i in range(desktop.get_child_count()):
                app = desktop.get_child_at_index(i)
                try:
                    if app and app.get_name() == "stet" and app.get_process_id() == self.pid:
                        return app
                except GLib.Error:
                    continue
            return None
        app = wait_for(lookup, timeout, what="Stet on the accessibility bus")
        return app

    def find(self, predicate, root=None):
        def walk(node, depth=0):
            if node is None or depth > 40:
                return None
            if alive(predicate, node):
                return node
            for child in children(node):
                found = walk(child, depth + 1)
                if found:
                    return found
            return None
        return walk(root or self.accessible())

    def showing(self, role, name=None):
        roles = ("push button", "button") if role in ("push button", "button") else (role,)

        def matches(node):
            if node.get_role_name() not in roles:
                return False
            if name is not None and name not in (node.get_name() or ""):
                return False
            return node.get_state_set().contains(Atspi.StateType.SHOWING)
        return self.find(matches)

    def focused(self):
        node = self.find(lambda n: n.get_state_set().contains(Atspi.StateType.FOCUSED))
        return (node.get_role_name(), node.get_name() or "") if node else (None, "")

    def count_showing(self, *roles):
        count = 0

        def walk(node, depth=0):
            nonlocal count
            if node is None or depth > 40:
                return
            if alive(lambda n: n.get_role_name() in roles
                     and n.get_state_set().contains(Atspi.StateType.SHOWING), node):
                count += 1
            for child in children(node):
                walk(child, depth + 1)
        walk(self.accessible())
        return count

    def editor(self):
        node = self.showing("text")
        if not node:
            raise Failed("no visible editor text view")
        return node

    def text(self):
        return Atspi.Text.get_text(self.editor(), 0, -1)

    def caret(self):
        label = self.find(lambda n: n.get_role_name() == "label"
                          and (n.get_name() or "").startswith("Ln "))
        return label.get_name() if label else ""

    def char_height(self):
        extents = Atspi.Text.get_character_extents(self.editor(), 0, Atspi.CoordType.WINDOW)
        return extents.height

    def press_button(self, label):
        button = self.find(lambda n: n.get_role_name() in ("push button", "button")
                           and label.lower() in (n.get_name() or "").lower()
                           and n.get_state_set().contains(Atspi.StateType.SHOWING))
        if not button:
            raise Failed(f"no visible button {label!r}")
        Atspi.Action.do_action(button, 0)
        time.sleep(0.3)

    def stop(self):
        if self.proc is None:
            if self.alive():
                os.kill(self.pid, signal.SIGTERM)
                try:
                    wait_for(lambda: not self.alive(), timeout=5, what="the attached Stet to quit")
                except Failed:
                    os.kill(self.pid, signal.SIGKILL)
            return
        if self.proc.poll() is None:
            self.proc.send_signal(signal.SIGTERM)
            try:
                self.proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait()


# ---------------------------------------------------------------------------------- checks

CHECKS = {}
OPT_IN = {"live_selftests", "dbus_service"}


def check(function):
    CHECKS[function.__name__.removeprefix("check_")] = function
    return function


@check
def check_startup(ctx):
    """Median time from main() to the first painted frame over 5 launches."""
    times = []
    for _ in range(5):
        stet = Stet(env={"STET_EXIT_AFTER_MS": "800"}, wait_window=False)
        stet.proc.wait(timeout=15)
        line = next((l for l in stet.lines if "first-frame" in l), "")
        times.append(float(line.rsplit(" ", 1)[-1]) if line else float("inf"))
        time.sleep(0.4)
    median = statistics.median(times)
    if median > 400:
        raise Failed(f"median {median:.1f} ms > 400 ms ({times})")
    return f"median {median:.1f} ms (min {min(times):.1f}, max {max(times):.1f})"


@check
def check_second_launch(ctx):
    """A second launch focuses the running window and opens FILE:LINE as a tab."""
    first, second, third = (ctx.file(f"launch-{n}.txt", "".join(f"line {i}\n" for i in range(1, 61)))
                            for n in "abc")
    stet = ctx.track(Stet(first))
    focus(ctx.original)
    wait_for(lambda: active()[0] != APP_ID, what="focus to move away")
    subprocess.run([str(EXE), f"{second}:40"], env=stet_env(), timeout=10, check=True)
    wait_for(lambda: active()[0] == APP_ID, what="focus on Stet after a direct second launch")
    wait_for(lambda: "launch-b.txt" in stet.title(), what="the second file's tab")
    wait_for(lambda: stet.caret().startswith("Ln 40,"), what="the caret on line 40")
    focus(ctx.original)
    wait_for(lambda: active()[0] != APP_ID, what="focus to move away")
    sandbox = " ".join(f"{name}={value}" for name, value in SANDBOX.items())
    lua(f'hl.dsp.exec_cmd("env STET_APP_ID={APP_ID} {sandbox} {EXE} {third}")')
    wait_for(lambda: active()[0] == APP_ID, what="focus on Stet after a compositor launch")
    wait_for(lambda: "launch-c.txt" in stet.title(), what="the third file's tab")
    return "focus moved to Stet for a shell launch and a compositor launch; FILE:40 put the caret on line 40"


def sandbox_theme(directory, theme):
    current = directory / "current"
    (current / "theme").mkdir(parents=True, exist_ok=True)
    shutil.copy(THEMES / theme / "colors.toml", current / "theme" / "colors.toml")
    (current / "theme.name").write_text(theme + "\n")


def swap_theme(directory, theme):
    """What omarchy-theme-set does: build next-theme, replace theme, write theme.name."""
    current = directory / "current"
    staged = current / "next-theme"
    staged.mkdir()
    shutil.copy(THEMES / theme / "colors.toml", staged / "colors.toml")
    shutil.rmtree(current / "theme")
    os.rename(staged, current / "theme")
    (current / "theme.name").write_text(theme + "\n")


def theme_background(theme):
    text = (THEMES / theme / "colors.toml").read_text()
    match = re.search(r'^background\s*=\s*"(#[0-9a-fA-F]{6})"', text, flags=re.M)
    return hex_rgb(match.group(1))


@check
def check_theme_switch(ctx):
    """The editor recolours live when the (sandboxed) Omarchy theme changes."""
    state = ctx.dir("omarchy-state")
    sandbox_theme(state, "tokyo-night")
    stet = ctx.track(Stet(ctx.file("theme.txt", "theme check\n"), env={"STET_OMARCHY_STATE_DIR": str(state)}))
    stet.wait_log("theme applied")
    client = stet.window()
    x = client["at"][0] + client["size"][0] * 3 // 4
    y = client["at"][1] + client["size"][1] * 3 // 5
    time.sleep(0.5)
    dark = pixel(x, y)
    if not near(dark, theme_background("tokyo-night")):
        raise Failed(f"editor background {dark} is not tokyo-night's {theme_background('tokyo-night')}")
    swapped = time.monotonic()
    swap_theme(state, "catppuccin-latte")
    stet.wait_log("theme applied", count=2, timeout=4)
    reload_ms = (time.monotonic() - swapped) * 1000
    latte = theme_background("catppuccin-latte")
    light = wait_for(lambda: (lambda p: p if near(p, latte) else None)(pixel(x, y)),
                     timeout=2, what="the catppuccin-latte background")
    return f"tokyo-night {dark} → catppuccin-latte {light} in {reload_ms:.0f} ms"


@check
def check_theme_refresh(ctx):
    """`omarchy-theme-refresh` (the real Omarchy state) makes a running Stet re-apply the theme."""
    stet = ctx.track(Stet(ctx.file("refresh.txt", "refresh check\n")))
    stet.wait_log("theme applied")
    subprocess.run(["omarchy-theme-refresh"], check=True, timeout=60,
                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    wait_for(lambda: stet.log_count("theme applied") >= 2 or stet.log_count("theme reloaded; unchanged"),
             timeout=6, what="Stet to reload the theme")
    outcome = "re-applied" if stet.log_count("theme applied") >= 2 else "reloaded (palette unchanged)"
    return f"omarchy-theme-refresh made Stet reload: {outcome}"


# Exactly what omarchy-font-set writes to ~/.config/fontconfig/fonts.conf.
FONTS_CONF = """<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "fonts.dtd">
<fontconfig>
  <match target="pattern">
    <test name="family" qual="any">
      <string>monospace</string>
    </test>
    <edit name="family" mode="prepend_first" binding="strong">
      <string>{}</string>
    </edit>
  </match>
</fontconfig>
"""


@check
def check_font_change(ctx):
    """The editor follows a live monospace change (sandboxed fontconfig, not the user's)."""
    config = ctx.dir("xdg-config")
    conf = config / "fontconfig" / "fonts.conf"
    conf.parent.mkdir(parents=True)
    conf.write_text(FONTS_CONF.format("Liberation Mono"))
    stet = ctx.track(Stet(ctx.file("font.txt", "a\tb\tc\n"), env={"XDG_CONFIG_HOME": str(config)}))
    stet.wait_log("editor font family=Liberation Mono")
    conf.write_text(FONTS_CONF.format("Adwaita Mono"))
    stet.wait_log("editor font family=Adwaita Mono", timeout=10)
    return "Liberation Mono → Adwaita Mono applied live"


@check
def check_drag_and_drop(ctx):
    """A file dragged from another GTK4 app (like Nautilus) opens as a tab; no URI text inserted."""
    original = "keep me\n"
    stet = ctx.track(Stet(ctx.file("drop-target.txt", original)))
    dropped = ctx.file("dropped.txt", "dropped content\n")
    source = ctx.track_process(subprocess.Popen([sys.executable, str(DRAG_SOURCE), str(dropped)]))
    src = wait_for(lambda: next((c for c in hypr("clients") if c.get("title") == "Stet drag source"), None),
                   what="the drag source window")
    time.sleep(0.8)
    src = next(c for c in hypr("clients") if c.get("title") == "Stet drag source")
    target = stet.window()
    centre = lambda c: (c["at"][0] + c["size"][0] // 2, c["at"][1] + c["size"][1] // 2)
    drag(*centre(src), *centre(target))
    wait_for(lambda: "dropped.txt" in stet.title(), what="the dropped file's tab")
    if stet.text() != "dropped content\n":
        raise Failed(f"the dropped tab shows {stet.text()!r}")
    lua(f'hl.dsp.window.close({{ window = "address:{src["address"]}" }})')
    source.wait(timeout=5)
    return "the dropped file opened as the active tab"


@check
def check_typing_focus(ctx):
    """After opening a file, the editor has keyboard focus: typed text lands in the document."""
    stet = ctx.track(Stet(ctx.file("focus.txt", "focus\n")))
    wait_for(lambda: active()[0] == APP_ID, what="focus on Stet")
    time.sleep(0.4)
    type_text("z")
    wait_for(lambda: stet.text().startswith("z"), timeout=2,
             what="the typed z in the document (keyboard focus on the editor)")
    return "typing after opening a file went into the editor"


@check
def check_universal_clipboard(ctx):
    """SUPER+C/V/X as Omarchy sends them (Ctrl+C/V/X injected by Hyprland) work in the editor."""
    stet = ctx.track(Stet(ctx.file("clip.txt", "alpha beta")))
    stet.window()
    wait_for(lambda: active()[0] == APP_ID, what="focus on Stet")
    editor = stet.editor()
    Atspi.Text.set_selection(editor, 0, 0, 10)
    paths = [universal_shortcut("C")]
    wait_for(lambda: clipboard() == "alpha beta", what="the copied text on the clipboard")
    Atspi.Text.set_caret_offset(editor, 10)
    paths.append(universal_shortcut("V"))
    wait_for(lambda: stet.text() == "alpha betaalpha beta", what="the pasted text")
    Atspi.Text.set_selection(stet.editor(), 0, 0, 20)
    paths.append(universal_shortcut("X"))
    wait_for(lambda: stet.text() == "", what="the cut to empty the editor")
    if clipboard() != "alpha betaalpha beta":
        raise Failed(f"the clipboard holds {clipboard()!r} after cut")
    how = "Hyprland's injected Ctrl keys" if set(paths) == {"compositor"} else f"Ctrl keys ({', '.join(paths)})"
    return f"copy, paste and cut through {how}"


@check
def check_shortcuts(ctx):
    """F-key-free shortcuts: palette, find next/previous, full screen, zoom, quit."""
    stet = ctx.track(Stet(ctx.file("keys.txt", "alpha beta gamma\nbeta delta\n")))
    for chord in ("ctrl+shift+p", "F1"):
        keys(chord)
        wait_for(lambda: stet.focused()[0] not in (None, "text"), what=f"{chord} to open the palette")
        keys("Escape")
        wait_for(lambda: stet.focused()[0] == "text", what="focus back on the editor")
    keys("ctrl+f")
    type_text("beta")
    keys("Escape")
    keys("ctrl+Home")
    # "Sel 4" (M1) or "Sel 4 | 1" (M5: characters, then lines).
    on = lambda line: lambda: (stet.caret().startswith(f"Ln {line},")
                               and re.search(r"Sel 4(?: \| 1)?$", stet.caret()) is not None)
    keys("alt+Down")
    wait_for(on(1), what="Alt+Down to select the first match on line 1")
    keys("alt+Down")
    wait_for(on(2), what="Alt+Down to select the next match on line 2")
    keys("alt+Up")
    wait_for(on(1), what="Alt+Up to go back to the match on line 1")
    keys("alt+Return")
    wait_for(lambda: (stet.client() or {}).get("fullscreen"), what="full screen on")
    keys("alt+Return")
    wait_for(lambda: not (stet.client() or {}).get("fullscreen"), what="full screen off")
    before = stet.char_height()
    keys("ctrl+plus")
    wait_for(lambda: stet.char_height() > before, what="Ctrl++ to zoom in")
    keys("ctrl+minus")
    keys("ctrl+minus")
    wait_for(lambda: stet.char_height() < before, what="Ctrl+- to zoom out")
    keys("ctrl+0")
    wait_for(lambda: stet.char_height() == before, what="Ctrl+0 to reset the zoom")
    keys("ctrl+alt+q")
    stet.proc.wait(timeout=5)
    return "palette, Alt+Down/Up, Alt+Enter, Ctrl++/-/0 and Ctrl+Alt+Q all worked"


@check
def check_file_choosers(ctx):
    """Open and Save As choosers appear floating and close with Escape."""
    stet = ctx.track(Stet(ctx.file("chooser.txt", "chooser\n")))
    results = []
    for combo, what in (("ctrl+o", "Open"), ("ctrl+alt+s", "Save As")):
        keys(combo)
        portal = wait_for(lambda: next((c for c in hypr("clients") if c.get("class") == PORTAL), None),
                          what=f"the {what} chooser")
        if not portal.get("floating"):
            raise Failed(f"the {what} chooser is tiled")
        wait_for(lambda: active()[0] == PORTAL, what=f"focus on the {what} chooser")
        keys("Escape", target=PORTAL)
        wait_for(lambda: not any(c.get("class") == PORTAL for c in hypr("clients")),
                 what=f"the {what} chooser to close")
        wait_for(lambda: active()[0] == APP_ID, what="focus back on Stet")
        results.append(f"{what} {portal['size'][0]}×{portal['size'][1]} floating")
    return "; ".join(results)


@check
def check_unsaved_prompts(ctx):
    """Ctrl+W on a dirty tab asks; closing the window (SUPER+W) doesn't, and the draft comes
    back on the next start (M2)."""
    stet = ctx.track(Stet())
    type_text("unsaved")
    keys("ctrl+w")
    wait_for(lambda: stet.showing("push button", "Discard") or stet.showing("push button", "Without"),
             what="the save prompt for Ctrl+W")
    stet.press_button("Cancel")
    if stet.text() != "unsaved":
        raise Failed("cancelling the prompt changed the text")
    started = time.monotonic()
    lua(f'hl.dsp.window.close({{ window = "address:{stet.window()["address"]}" }})')
    try:
        stet.proc.wait(timeout=10)
    except subprocess.TimeoutExpired:
        raise Failed("closing the window did not quit (a prompt?)") from None
    quit_ms = (time.monotonic() - started) * 1000
    again = ctx.track(Stet())
    wait_for(lambda: again.text() == "unsaved", what="the draft restored on the next start")
    return (f"Ctrl+W asked and Cancel kept the text; SUPER+W quit without asking in {quit_ms:.0f} ms"
            " and the draft came back")


@check
def check_session_signals(ctx):
    """SIGTERM right after typing keeps the typing; after kill -9 only the typing since the
    last 7 s backup can be lost (M2)."""
    path = ctx.file("signals.txt", "base\n")
    stet = ctx.track(Stet(path))
    type_text("t")
    wait_for(lambda: stet.text() == "tbase\n", what="the typed t")
    stet.proc.send_signal(signal.SIGTERM)
    stet.proc.wait(timeout=10)
    if not any("SIGTERM" in line for line in stet.lines):
        raise Failed("Stet did not log the SIGTERM")
    second = ctx.track(Stet())
    wait_for(lambda: second.text() == "tbase\n", what="the typing kept through SIGTERM")
    type_text("k")
    kept = wait_for(lambda: (lambda t: t if t != "tbase\n" else None)(second.text()),
                    what="the typed k")
    time.sleep(8)
    type_text("z")
    late = wait_for(lambda: (lambda t: t if t != kept else None)(second.text()), what="the typed z")
    second.proc.kill()
    second.proc.wait(timeout=5)
    third = ctx.track(Stet())
    restored = wait_for(lambda: (lambda t: t if t in (kept, late) else None)(third.text()),
                        timeout=8, what=f"the text from the last backup ({kept!r} or {late!r})")
    which = "including the last key (a backup ran in between)" if restored == late else \
        "without the key typed just before the kill"
    return f"SIGTERM kept the typing; kill -9 restored the backup from 8 s earlier, {which}"


def git(repo, *args, env=None):
    return subprocess.run(["git", "-C", str(repo), *args], env=env, capture_output=True,
                          text=True, timeout=60)


def scratch_repo(ctx):
    repo = ctx.dir("repo")
    git(repo, "init", "-q", "-b", "main")
    git(repo, "config", "user.name", "Stet live test")
    git(repo, "config", "user.email", "stet-live-test@invalid")
    (repo / "file.txt").write_text("one\n")
    git(repo, "add", "file.txt")
    return repo


def waited_stet(ctx, before):
    """The background Stet that a `--wait` started: its window shows up with a new pid."""
    client = wait_for(lambda: next((c for c in hypr("clients") if c.get("class") == APP_ID
                                    and c.get("pid") not in before), None),
                      timeout=15, what="the window of the Stet that --wait started")
    stet = ctx.track(Stet.attach(client["pid"]))
    stet.window()
    return stet


def commit_with(ctx, repo, message=None):
    """`git commit` with GIT_EDITOR="stet --wait"; types `message` into the commit message and
    saves and closes the tab, or types something and closes the tab with Discard when `message`
    is None (`--wait` then exits 1 and git aborts)."""
    env = stet_env({"GIT_EDITOR": f"{EXE} --wait"})
    before = {c.get("pid") for c in hypr("clients") if c.get("class") == APP_ID}
    commit = ctx.track_process(subprocess.Popen(["git", "-C", str(repo), "commit", "-q"], env=env,
                                                stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                                text=True))
    running = [c for c in hypr("clients") if c.get("class") == APP_ID]
    if running:
        stet = Stet.attach(running[0]["pid"])
        wait_for(lambda: "COMMIT_EDITMSG" in stet.title(), timeout=15, what="the commit message tab")
        stet.window()
    else:
        stet = waited_stet(ctx, before)
        wait_for(lambda: "COMMIT_EDITMSG" in stet.title(), timeout=15, what="the commit message tab")
    if message is None:
        # A clean tab closes without a prompt and git would see an empty message; type first.
        keys("ctrl+Home")
        type_text("discarded\n")
        keys("ctrl+w")
        wait_for(lambda: stet.showing("push button", "Discard"), what="the save prompt")
        stet.press_button("Discard")
    else:
        keys("ctrl+Home")
        type_text(message + "\n")
        keys("ctrl+s")
        wait_for(lambda: "COMMIT_EDITMSG" in stet.title() and not stet.title().startswith("*"),
                 what="the saved commit message")
        keys("ctrl+w")
    output, _ = commit.communicate(timeout=20)
    return stet, commit.returncode, output.strip()


@check
def check_git_editor_wait(ctx):
    """GIT_EDITOR="stet --wait" with Stet not running (it starts one in the background) and
    running: saving and closing the tab commits, Discard aborts the commit (M2)."""
    repo = scratch_repo(ctx)
    for client in hypr("clients"):
        if client.get("class") == APP_ID:
            raise Failed("a Stet with the test app-id is already running")
    stet, code, output = commit_with(ctx, repo, "first commit from stet")
    if code != 0:
        raise Failed(f"git commit exited {code}: {output}")
    subject = git(repo, "log", "-1", "--format=%s").stdout.strip()
    if subject != "first commit from stet":
        raise Failed(f"the commit's subject is {subject!r}")
    if not stet.alive():
        raise Failed("the background Stet quit when the waited tab closed")
    (repo / "file.txt").write_text("two\n")
    git(repo, "add", "file.txt")
    _, code, output = commit_with(ctx, repo, None)
    if code == 0:
        raise Failed("Discard did not abort the commit")
    count = git(repo, "rev-list", "--count", "HEAD").stdout.strip()
    if count != "1":
        raise Failed(f"the repository has {count} commits after the aborted one")
    return ("not running: --wait started Stet in the background and the saved message was"
            " committed; running: Discard aborted the commit")


SERVICE_DIR = Path.home() / ".local/share/dbus-1/services"


def reload_bus():
    subprocess.run(["busctl", "--user", "call", "org.freedesktop.DBus", "/org/freedesktop/DBus",
                    "org.freedesktop.DBus", "ReloadConfig"], check=True, capture_output=True, timeout=10)


def stet_units():
    out = subprocess.run(["systemctl", "--user", "list-units", "--plain", "--no-legend", "--all",
                          f"*{APP_ID}*"], capture_output=True, text=True, timeout=10).stdout
    return [line.split()[0] for line in out.splitlines() if line.strip()]


@check
def check_dbus_service(ctx):
    """Opt-in: with a D-Bus service file for the test app-id (installed for the check, then
    removed), `stet FILE` starts Stet in its own systemd unit, `systemctl --user stop` keeps
    the typing, and `stet --wait` works through the bus (M2)."""
    service = SERVICE_DIR / f"{APP_ID}.service"
    if service.exists():
        raise Failed(f"{service} exists already; not touching it")
    if any(c.get("class") == APP_ID for c in hypr("clients")) or stet_units():
        raise Failed("a Stet with the test app-id is already running")
    variables = " ".join(f"{name}={value}" for name, value in SANDBOX.items())
    created = [d for d in (SERVICE_DIR.parent, SERVICE_DIR) if not d.exists()]
    SERVICE_DIR.mkdir(parents=True, exist_ok=True)
    service.write_text(f"[D-BUS Service]\nName={APP_ID}\n"
                       f"Exec=/usr/bin/env STET_APP_ID={APP_ID} RUST_LOG=info {variables}"
                       f" {EXE} --gapplication-service\n")
    try:
        reload_bus()
        path = ctx.file("service.txt", "service\n")
        before = {c.get("pid") for c in hypr("clients") if c.get("class") == APP_ID}
        subprocess.run([str(EXE), str(path)], env=stet_env(), timeout=20, check=True)
        stet = waited_stet(ctx, before)
        units = wait_for(stet_units, what="the systemd unit of the activated Stet")
        wait_for(lambda: "service.txt" in stet.title(), what="the file's tab")
        type_text("u")
        wait_for(lambda: stet.text() == "uservice\n", what="the typed u")
        subprocess.run(["systemctl", "--user", "stop", units[0]], check=True, timeout=20)
        wait_for(lambda: not stet.alive(), timeout=10, what="the unit to stop")
        before = {c.get("pid") for c in hypr("clients") if c.get("class") == APP_ID}
        subprocess.run([str(EXE)], env=stet_env(), timeout=20, check=True)
        again = waited_stet(ctx, before)
        wait_for(lambda: again.text() == "uservice\n", what="the typing kept through systemctl stop")
        repo = scratch_repo(ctx)
        _, code, output = commit_with(ctx, repo, "commit through the bus")
        if code != 0:
            raise Failed(f"git commit exited {code}: {output}")
        again.stop()
        return (f"activated as {units[0].split('@')[0]}…; systemctl --user stop kept the typing;"
                " --wait through the bus committed")
    finally:
        service.unlink(missing_ok=True)
        for directory in reversed(created):
            directory.rmdir()
        reload_bus()
        for unit in stet_units():  # the empty slice systemd made for the activated unit
            subprocess.run(["systemctl", "--user", "stop", unit], capture_output=True, timeout=20)


def caret_to(stet, offset):
    editor = stet.editor()
    Atspi.Text.set_caret_offset(editor, offset)
    wait_for(lambda: Atspi.Text.get_caret_offset(editor) == offset, what=f"the caret at {offset}")


@check
def check_toolbox_keys(ctx):
    """M5's editor keys on the real keymap: line operations, case, comments, blank lines, the
    whole-line clipboard, Select and Find Next/Previous, brace jumps and the JSON tools."""
    original = "alpha\nbeta\ngamma\n"
    stet = ctx.track(Stet(ctx.file("tools.rs", original)))
    editor = stet.editor()
    done = []

    def expect(keys_, text, at=0):
        caret_to(stet, at)
        keys(keys_)
        wait_for(lambda: stet.text() == text, what=f"{keys_} to give {text!r}")
        keys("ctrl+z")
        wait_for(lambda: stet.text() == original, what=f"Ctrl+Z to undo {keys_} in one step")
        done.append(keys_)

    expect("ctrl+d", "alpha\nalpha\nbeta\ngamma\n")
    expect("ctrl+shift+Down", "beta\nalpha\ngamma\n")
    expect("ctrl+shift+Up", "beta\nalpha\ngamma\n", at=6)
    expect("ctrl+shift+l", "beta\ngamma\n")
    expect("ctrl+t", "beta\nalpha\ngamma\n", at=6)
    expect("ctrl+j", "alpha beta\ngamma\n")
    expect("ctrl+alt+Return", "\nalpha\nbeta\ngamma\n")
    expect("alt+shift+u", "ALPHA\nbeta\ngamma\n", at=2)
    expect("alt+u", "Alpha\nbeta\ngamma\n", at=2)
    caret_to(stet, 0)
    keys("ctrl+q")
    wait_for(lambda: stet.text().startswith("//") and stet.text().endswith("alpha\nbeta\ngamma\n"),
             what="Ctrl+Q to comment the line")
    keys("ctrl+q")
    wait_for(lambda: stet.text() == original, what="Ctrl+Q again to uncomment it")
    done.append("ctrl+q")
    caret_to(stet, 7)
    keys("ctrl+c")
    wait_for(lambda: clipboard() == "beta\n", what="Ctrl+C without a selection to copy the line")
    keys("ctrl+x")
    wait_for(lambda: stet.text() == "alpha\ngamma\n", what="Ctrl+X without a selection to cut the line")
    keys("ctrl+z")
    wait_for(lambda: stet.text() == original, what="undo of the cut")
    caret_to(stet, 13)
    keys("ctrl+shift+x")
    wait_for(lambda: clipboard() == "gamma\n", what="Ctrl+Shift+X to copy the line")
    done += ["ctrl+c", "ctrl+x", "ctrl+shift+x"]

    stet.stop()
    words = "alpha beta alpha\n"
    stet = ctx.track(Stet(ctx.file("words.txt", words)))
    editor = stet.editor()
    caret_to(stet, 1)
    keys("ctrl+alt+f")
    wait_for(lambda: Atspi.Text.get_selection(editor, 0).start_offset == 11
             and Atspi.Text.get_selection(editor, 0).end_offset == 16,
             what="Ctrl+Alt+F to select the next alpha")
    keys("ctrl+shift+F3")
    wait_for(lambda: Atspi.Text.get_selection(editor, 0).start_offset == 0
             and Atspi.Text.get_selection(editor, 0).end_offset == 5,
             what="Ctrl+Shift+F3 to select the previous alpha")
    done += ["ctrl+alt+f", "ctrl+shift+F3"]

    stet.stop()
    braces = "fn f() { g(); }\n"
    stet = ctx.track(Stet(ctx.file("braces.rs", braces)))
    editor = stet.editor()
    caret_to(stet, 7)
    keys("ctrl+b")
    wait_for(lambda: Atspi.Text.get_caret_offset(editor) in (14, 15),
             what="Ctrl+B to jump to the matching brace")
    done.append("ctrl+b")

    stet.stop()
    stet = ctx.track(Stet(ctx.file("data.json", '{"a":[1,2]}')))
    keys("ctrl+alt+shift+m")
    wait_for(lambda: '"a": [' in stet.text() and "\n" in stet.text(), what="Ctrl+Alt+Shift+M to format JSON")
    keys("ctrl+alt+shift+j")
    wait_for(lambda: shows(stet, "Valid JSON"), what="Ctrl+Alt+Shift+J to report valid JSON")
    done += ["ctrl+alt+shift+m", "ctrl+alt+shift+j"]
    return f"{len(done)} keys did their job and each undid in one step: " + ", ".join(done)


def screen_centre(stet, node):
    client = stet.client()
    box = node.get_extents(Atspi.CoordType.WINDOW)
    return client["at"][0] + box.x + box.width // 2, client["at"][1] + box.y + box.height // 2


def pointer(action, x, y):
    """A pointer action at screen coordinates; the pointer first moves next to the spot, so the
    surface sees it arrive, and a new window has had time to finish its open animation."""
    monitor = next(m for m in hypr("monitors") if m.get("focused"))
    tool = [str(POINTER), "--extent", str(monitor["width"]), str(monitor["height"])]
    subprocess.run([*tool, "move", str(x - 8), str(y)], check=True, timeout=10)
    time.sleep(0.2)
    subprocess.run([*tool, action, str(x), str(y)], check=True, timeout=10)


# The tab menu's items in order for an unpinned tab (TAB_MENU: M8's Pin Tab, which shows
# instead of Unpin Tab, then M5's, with Save and Save As… since 1.1 and Rename File… called
# Rename… since 1.2). GTK 4.22's popover menu items have no accessible name over AT-SPI, so
# they are found by position.
TAB_MENU = ["Pin Tab", "Close", "Close Others", "Close to the Right", "Save", "Save As…", "Copy Full Path",
            "Copy File Name", "Copy Directory Path", "Open Containing Folder", "Open Terminal Here",
            "Rename…", "Move to Trash…"]


def menu_items(stet):
    found = []

    def walk(node, depth=0):
        for child in children(node):
            if alive(lambda n: n.get_role_name() == "menu item"
                     and n.get_state_set().contains(Atspi.StateType.SHOWING), child):
                found.append(child)
            if depth < 40:
                walk(child, depth + 1)
    walk(stet.accessible())
    return found


def tab_menu_item(stet, label):
    items = wait_for(lambda: (lambda found: found if len(found) == len(TAB_MENU) else None)(
        menu_items(stet)), what=f"the tab menu's {len(TAB_MENU)} items")
    return items[TAB_MENU.index(label)]


def open_tab_menu(stet, name):
    tab = wait_for(lambda: stet.find(lambda n: n.get_role_name() == "page tab"
                                     and name in (n.get_name() or "")), what=f"the tab {name}")
    time.sleep(0.8)
    pointer("right-click", *screen_centre(stet, tab))
    wait_for(lambda: stet.find(lambda n: n.get_role_name() == "menu item"
                               and n.get_state_set().contains(Atspi.StateType.SHOWING)),
             what="the tab menu")


@check
def check_tab_menu(ctx):
    """M5's tab context menu with the real mouse: Copy File Name, Save, Rename… with Enter, Open
    Terminal Here and Open Containing Folder (their windows closed again), Move to Trash… into
    a sandboxed trash; and the editor's context menu with the text tools."""
    scratch = ctx.home_dir()
    trash_home = scratch / "xdg-data"
    trash_home.mkdir()
    path = scratch / "menu-me.txt"
    path.write_text("menu\n")
    stet = ctx.track(Stet(path, env={"XDG_DATA_HOME": str(trash_home)}))
    done = []

    open_tab_menu(stet, "menu-me.txt")
    Atspi.Action.do_action(tab_menu_item(stet, "Copy File Name"), 0)
    wait_for(lambda: clipboard() == "menu-me.txt", what="the file name on the clipboard")
    done.append("Copy File Name")

    type_text("saved ")
    wait_for(lambda: stet.text() == "saved menu\n", what="the typed text")
    open_tab_menu(stet, "menu-me.txt")
    Atspi.Action.do_action(tab_menu_item(stet, "Save"), 0)
    wait_for(lambda: path.read_text() == "saved menu\n", what="the file saved from the tab menu")
    done.append("Save")

    open_tab_menu(stet, "menu-me.txt")
    Atspi.Action.do_action(tab_menu_item(stet, "Rename…"), 0)
    wait_for(lambda: stet.showing("push button", "Rename"), what="the Rename File dialog")
    time.sleep(0.3)
    type_text("renamed")
    keys("Return")
    renamed = scratch / "renamed.txt"
    wait_for(lambda: renamed.exists() and not path.exists(), what="the file renamed on disk")
    wait_for(lambda: "renamed.txt" in stet.title(), what="the window title to follow the rename")
    done.append("Rename… (the name without its extension selected, Enter applied)")

    for label, what in (("Open Terminal Here", "terminal"), ("Open Containing Folder", "file manager")):
        before = {c["address"] for c in hypr("clients")}
        open_tab_menu(stet, "renamed.txt")
        Atspi.Action.do_action(tab_menu_item(stet, label), 0)
        new = wait_for(lambda: next((c for c in hypr("clients") if c["address"] not in before
                                     and c.get("class") != APP_ID), None),
                       timeout=10, what=f"the {what} window")
        lua(f'hl.dsp.window.close({{ window = "address:{new["address"]}" }})')
        wait_for(lambda: all(c["address"] != new["address"] for c in hypr("clients")),
                 what=f"the {what} window to close")
        done.append(f"{label} ({new.get('class')})")
        stet.window()

    open_tab_menu(stet, "renamed.txt")
    Atspi.Action.do_action(tab_menu_item(stet, "Move to Trash…"), 0)
    wait_for(lambda: stet.showing("push button", "Move to Trash") or stet.showing("push button", "Trash"),
             what="the Move to Trash question")
    stet.press_button("Move to Trash") if stet.showing("push button", "Move to Trash") else stet.press_button("Trash")
    trashed = trash_home / "Trash" / "files" / "renamed.txt"
    wait_for(lambda: trashed.exists() and not renamed.exists(), what="the file in the sandboxed trash")
    done.append("Move to Trash… (sandboxed trash)")

    other = ctx.file("text-menu.txt", "case me\n")
    stet.stop()
    stet = ctx.track(Stet(other))
    time.sleep(0.8)
    pointer("right-click", *screen_centre(stet, stet.editor()))
    count = len(wait_for(lambda: menu_items(stet), what="the editor's context menu"))
    keys("Escape")
    done.append(f"the editor's context menu ({count} items)")
    return "; ".join(done)


@check
def check_tab_double_click(ctx):
    """A double-click on a tab with the real mouse: Rename… for a file's tab, Enter renames it
    on disk (1.1); for an untitled tab, a name for the tab, nothing saved (1.2)."""
    scratch = ctx.home_dir()
    path = scratch / "double-me.txt"
    path.write_text("double\n")
    stet = ctx.track(Stet(path))
    tab = wait_for(lambda: stet.find(lambda n: n.get_role_name() == "page tab"
                                     and "double-me.txt" in (n.get_name() or "")), what="the tab")
    time.sleep(0.8)
    pointer("double-click", *screen_centre(stet, tab))
    wait_for(lambda: stet.showing("push button", "Rename"), what="the Rename File dialog")
    time.sleep(0.3)
    type_text("clicked")
    keys("Return")
    renamed = scratch / "clicked.txt"
    wait_for(lambda: renamed.exists() and not path.exists(), what="the file renamed on disk")
    wait_for(lambda: "clicked.txt" in stet.title(), what="the window title to follow the rename")
    if renamed.read_text() != "double\n":
        raise Failed("the renamed file's text changed")
    done = ["a file's tab: Rename…, the name without its extension selected, Enter renamed it"]

    keys("ctrl+n")
    type_text("draft")
    tab = wait_for(lambda: stet.find(lambda n: n.get_role_name() == "page tab"
                                     and "draft" in (n.get_name() or "")), what="the untitled tab")
    time.sleep(0.8)
    pointer("double-click", *screen_centre(stet, tab))
    wait_for(lambda: stet.showing("push button", "Rename"), what="the Rename dialog for the tab")
    time.sleep(0.3)
    type_text("meeting")
    keys("Return")
    wait_for(lambda: stet.find(lambda n: n.get_role_name() == "page tab"
                               and "meeting" in (n.get_name() or "")), what="the tab named meeting")
    if any(c.get("class") == PORTAL for c in hypr("clients")):
        raise Failed("the untitled tab's double-click opened a file chooser")
    if stet.text() != "draft" or list(scratch.glob("meeting*")):
        raise Failed("naming the tab changed its text or saved a file")
    wait_for(lambda: active()[0] == APP_ID, what="focus back on Stet")
    done.append("an untitled tab: Rename…, the tab named “meeting” without saving")
    return "; ".join(done)


@check
def check_main_menu(ctx):
    """The hamburger menu at the left end of the tab strip (1.3): left of the first tab, opened
    with a real click; the Search submenu slides in, the menu widening to it (1.3.2); closed
    with Escape, and typing goes to the editor again."""
    stet = ctx.track(Stet(ctx.file("menu.txt", "menu\n")))
    button = wait_for(lambda: stet.find(lambda n: n.get_role_name() in ("toggle button", "push button")
                                        and (n.get_name() or "") == "Menu"), what="the menu button")
    tab = wait_for(lambda: stet.find(lambda n: n.get_role_name() == "page tab"
                                     and "menu.txt" in (n.get_name() or "")), what="the tab")
    menu_box = button.get_extents(Atspi.CoordType.WINDOW)
    tab_box = tab.get_extents(Atspi.CoordType.WINDOW)
    if menu_box.x + menu_box.width > tab_box.x:
        raise Failed(f"the menu ({menu_box.x}–{menu_box.x + menu_box.width}) is not left of the tab ({tab_box.x})")
    time.sleep(0.8)
    pointer("click", *screen_centre(stet, button))
    count = len(wait_for(lambda: menu_items(stet), what="the menu's items"))
    search = wait_for(lambda: next((item for item in menu_items(stet) if item.get_name() == "Search"), None),
                      what="the menu's Search item")
    Atspi.Action.do_action(search, 0)
    submenu = len(wait_for(lambda: (lambda items: items if any(item.get_name() == "Find…" for item in items)
                                    else None)(menu_items(stet)), what="the Search submenu's items"))
    keys("Escape")
    wait_for(lambda: not menu_items(stet), what="the menu to close")
    type_text("x")
    wait_for(lambda: stet.text() == "xmenu\n", what="typing in the editor after the menu closed")
    return (f"the menu at x {menu_box.x}–{menu_box.x + menu_box.width}, the first tab from {tab_box.x}; "
            f"a click opened it ({count} items), Search opened its submenu ({submenu} items), Escape closed it, "
            f"typing went to the editor")


def sel(stet):
    """The status bar's selection: (rows, columns) in column mode, else None."""
    match = re.search(r"Sel (\d+)×(\d+)", stet.caret())
    return (int(match.group(1)), int(match.group(2))) if match else None


@check
def check_column_keys(ctx):
    """M6 with real keys: Alt+Shift+arrows make a rectangle, typing goes in on every row and
    undoes in one step, Escape leaves; Alt+C's Column Editor inserts numbers."""
    original = "alpha\nbeta\ngamma\ndelta\n"
    stet = ctx.track(Stet(ctx.file("columns.txt", original)))
    caret_to(stet, 1)
    for key in ("alt+shift+Down", "alt+shift+Down", "alt+shift+Right", "alt+shift+Right"):
        keys(key)
    wait_for(lambda: sel(stet) == (3, 2), what="a 3×2 rectangle in the status bar")
    type_text("XY")
    wait_for(lambda: stet.text() == "aXYha\nbXYa\ngXYma\ndelta\n",
             what="XY typed over the rectangle on three rows")
    keys("ctrl+z")
    wait_for(lambda: stet.text() == original, what="Ctrl+Z to undo the burst in one step")
    keys("Escape")
    wait_for(lambda: sel(stet) is None, what="Escape to leave column mode")
    caret_to(stet, 0)
    keys("alt+c")
    wait_for(lambda: stet.showing("push button", "OK") or stet.showing("push button", "Insert"),
             what="the Column Editor")
    wait_for(lambda: stet.focused()[0] == "text", what="the focus in the Column Editor's first field")
    # Its radio buttons have no AT-SPI action; the keyboard is what users have anyway.
    keys("alt+n")
    for value in ("1", "1"):
        keys("Tab")
        keys("ctrl+a")
        type_text(value)
    keys("Return")
    wait_for(lambda: stet.text().startswith("1alpha\n2beta\n3gamma\n4delta"),
             what="numbers 1–4 down column 1")
    wait_for(lambda: stet.focused()[0] == "text", what="the focus back in the editor")
    return "Alt+Shift+arrows made a 3×2 rectangle; typing filled every row, one undo step; Alt+C numbered the lines"


@check
def check_column_clipboard(ctx):
    """M6: SUPER+C on a rectangle puts its rows on the clipboard as plain text for other apps."""
    stet = ctx.track(Stet(ctx.file("block.txt", "abcdef\nabc\nabcdef\n")))
    caret_to(stet, 2)
    for key in ("alt+shift+Down", "alt+shift+Down", "alt+shift+Right", "alt+shift+Right", "alt+shift+Right"):
        keys(key)
    wait_for(lambda: sel(stet) == (3, 3), what="a 3×3 rectangle")
    universal_shortcut("C")
    wait_for(lambda: clipboard() == "cde\nc\ncde", what="the block's rows on the clipboard")
    return "SUPER+C copied the 3×3 block as plain text rows"


@check
def check_a11y_add_selection(ctx):
    """M6's NULL-safe get_selection: AT-SPI Text.AddSelection on the editor with a selection
    (it crashed GTK 4.22, docs/upstream/gtk-accessible-text-add-selection.md)."""
    stet = ctx.track(Stet(ctx.file("a11y.txt", "select some of this\n")))
    editor = stet.editor()
    Atspi.Text.set_selection(editor, 0, 0, 6)
    time.sleep(0.3)
    try:
        Atspi.Text.add_selection(editor, 7, 11)
    except GLib.Error:
        pass
    time.sleep(1.0)
    if not stet.alive():
        raise Failed(f"Stet died (exit {stet.proc.returncode})")
    if stet.text() != "select some of this\n":
        raise Failed("the text changed")
    return "Text.AddSelection with a selection present: Stet kept running"


@check
def check_alt_drag_free(ctx):
    """M6: Hyprland binds no mouse button with Alt unless SUPER is held too, so Alt+drag and
    Alt+click reach Stet's column selection. (Holding Alt during an injected drag is not
    possible with send_key_state; the gesture itself is in the self-tests and manual row 24.)"""
    alt, super_ = 8, 64
    mouse = [b for b in hypr("binds") if b.get("mouse") or str(b.get("key", "")).startswith("mouse")]
    grabbing = [b for b in mouse if b.get("modmask", 0) & alt and not b.get("modmask", 0) & super_]
    if grabbing:
        raise Failed(f"Hyprland takes Alt+mouse: {[(b.get('modmask'), b.get('key')) for b in grabbing]}")
    mouse = sorted({(b.get("modmask"), b.get("key")) for b in mouse})
    return f"no Alt mouse binds; Hyprland's mouse binds are {mouse} (64 = SUPER, 72 = SUPER+Alt)"


def key_state(mods, key, state):
    lua(f'hl.dsp.send_key_state({{ mods = "{mods}", key = "{key}", state = "{state}" }})')
    time.sleep(0.03)


@check
def check_switcher(ctx):
    """M8's Ctrl+Tab with real key presses and releases: a quick tap goes to the previous tab;
    holding Ctrl over two Tabs goes two back in recent order; Ctrl+PgDn keeps strip order."""
    names = [f"tab{n}.txt" for n in range(1, 5)]
    paths = [ctx.file(name, f"{name}\n") for name in names]
    stet = ctx.track(Stet(*paths))
    wait_for(lambda: "tab4.txt" in stet.title(), what="tab4 in front")
    # Click every tab in strip order, so the recent order is tab4, tab3, tab2, tab1.
    for name in ("tab1.txt", "tab2.txt", "tab3.txt", "tab4.txt"):
        stet_tab = wait_for(lambda: stet.find(lambda n: n.get_role_name() == "page tab"
                                              and name in (n.get_name() or "")), what=name)
        time.sleep(0.2)
        pointer("click", *screen_centre(stet, stet_tab))
        wait_for(lambda: name in stet.title(), what=f"{name} in front")
    # A tap: Ctrl down, Tab, Ctrl up.
    require_active(APP_ID)
    key_state("CTRL", "Control_L", "down")
    key_state("CTRL", "Tab", "down")
    key_state("CTRL", "Tab", "up")
    key_state("", "Control_L", "up")
    wait_for(lambda: "tab3.txt" in stet.title(), what="a Ctrl+Tab tap to go back to tab3")
    # Held: Ctrl down, Tab, Tab, then Ctrl up: two back from tab3 in recent order is tab2.
    key_state("CTRL", "Control_L", "down")
    for _ in range(2):
        key_state("CTRL", "Tab", "down")
        key_state("CTRL", "Tab", "up")
        time.sleep(0.15)
    time.sleep(0.3)
    key_state("", "Control_L", "up")
    wait_for(lambda: "tab2.txt" in stet.title(), what="Ctrl held over two Tabs to reach tab2")
    keys("ctrl+Next")
    wait_for(lambda: "tab3.txt" in stet.title(), what="Ctrl+PgDn to the next tab in the strip")
    return "a tap went back to the previous tab; Ctrl held over two Tabs went two back; Ctrl+PgDn kept strip order"


@check
def check_back_forward(ctx):
    """M8: Go to Line leaves a place; Alt+Left and Alt+Right go back and forth, and so do the
    mouse's back and forward buttons."""
    stet = ctx.track(Stet(ctx.file("places.txt", "".join(f"line {n}\n" for n in range(1, 101)))))
    wait_for(lambda: stet.caret().startswith("Ln 1,"), what="the caret on line 1")
    keys("ctrl+g")
    wait_for(lambda: stet.focused()[0] not in (None, "text") or stet.showing("push button", "Go"),
             timeout=3, what="the Go to Line prompt")
    type_text("80")
    keys("Return")
    wait_for(lambda: stet.caret().startswith("Ln 80,"), what="Go to Line 80")
    keys("alt+Left")
    wait_for(lambda: stet.caret().startswith("Ln 1,"), what="Alt+Left back to line 1")
    keys("alt+Right")
    wait_for(lambda: stet.caret().startswith("Ln 80,"), what="Alt+Right forward to line 80")
    time.sleep(0.5)
    pointer("back", *screen_centre(stet, stet.editor()))
    wait_for(lambda: stet.caret().startswith("Ln 1,"), what="the mouse's back button to line 1")
    pointer("forward", *screen_centre(stet, stet.editor()))
    wait_for(lambda: stet.caret().startswith("Ln 80,"), what="the mouse's forward button to line 80")
    return "Alt+Left/Right and the mouse's back and forward buttons moved between line 80 and line 1"


def scratch_tree(ctx):
    repo = ctx.dir("tree")
    (repo / "src").mkdir()
    (repo / ".gitignore").write_text("target/\n")
    (repo / "target").mkdir()
    (repo / "target" / "ignored.txt").write_text("old ignored\n")
    (repo / "a.txt").write_text("old one\nold two\n")
    (repo / "b.txt").write_text("nothing here\n")
    (repo / "src" / "main.rs").write_text("fn main() {\n    let old = 1;\n}\n")
    (repo / "src" / "crlf.txt").write_bytes(b"old\r\nkeep\r\n")
    git(repo, "init", "-q", "-b", "main")
    git(repo, "add", "-A")
    git(repo, "-c", "user.name=Stet live test", "-c", "user.email=stet-live-test@invalid",
        "commit", "-q", "-m", "scratch")
    return repo


@check
def check_quick_open(ctx):
    """M8: Ctrl+P lists the project's files (the git root, .gitignore respected) and
    name:line opens a file at that line."""
    repo = scratch_tree(ctx)
    stet = ctx.track(Stet(repo / "a.txt"))
    keys("ctrl+p")
    wait_for(lambda: stet.focused()[0] not in (None,) and "a.txt" not in stet.focused()[1]
             and stet.focused()[0] in ("text", "entry"), timeout=3, what="the quick-open field")
    type_text("main:2")
    wait_for(lambda: shows(stet, "main.rs"), timeout=10, what="src/main.rs in the quick-open list")
    keys("Return")
    wait_for(lambda: "main.rs" in stet.title(), what="src/main.rs opened from quick open")
    wait_for(lambda: stet.caret().startswith("Ln 2,"), what="the caret on line 2")
    keys("ctrl+p")
    time.sleep(0.4)
    type_text("ignored")
    time.sleep(0.8)
    shown = shows(stet, "ignored.txt")
    keys("Escape")
    if shown:
        raise Failed("quick open listed target/ignored.txt, which .gitignore leaves out")
    return "Ctrl+P then main:2 opened src/main.rs at line 2; target/ (in .gitignore) was not listed"


@check
def check_replace_in_files(ctx):
    """M8 on a scratch git tree: Replace in Files rewrites only the matches (CRLF kept), saves
    the open clean file (Undo still there), and leaves an open dirty file unsaved."""
    repo = scratch_tree(ctx)
    stet = ctx.track(Stet(repo / "a.txt", repo / "src" / "main.rs"))
    wait_for(lambda: "main.rs" in stet.title(), what="main.rs in front")
    caret_to(stet, 0)
    type_text("x")
    wait_for(lambda: stet.text().startswith("xfn main"), what="main.rs made dirty")
    keys("ctrl+shift+f")
    wait_for(lambda: stet.focused()[0] == "entry", what="the find field")
    keys("ctrl+a")
    type_text("old")
    fields = [n for n in menu_items_of(stet, ("text",)) if alive(lambda n: Atspi.Text.get_text(n, 0, -1) == "", n)]
    if not fields:
        raise Failed("no empty replace field")
    pointer("click", *screen_centre(stet, fields[0]))
    type_text("new")
    button = wait_for(lambda: stet.showing("push button", "Replace in Files"), what="the Replace in Files button")
    # The folder field follows the current file (src/); search the whole tree instead.
    folder = stet.find(lambda n: n.get_role_name() == "text" and alive(
        lambda m: Atspi.Text.get_text(m, 0, -1).endswith("/src"), n))
    if folder:
        pointer("click", *screen_centre(stet, folder))
        keys("ctrl+a")
        type_text(str(repo))
    Atspi.Action.do_action(button, 0)
    wait_for(lambda: stet.showing("push button", "Replace in Files") and stet.showing("push button", "Cancel"),
             what="the Replace in Files question")
    time.sleep(0.3)
    confirm = [n for n in menu_items_of(stet, ("push button", "button"))
               if alive(lambda m: (m.get_name() or "") == "Replace in Files", n)]
    Atspi.Action.do_action(confirm[-1], 0)
    a = repo / "a.txt"
    wait_for(lambda: a.read_text() == "new one\nnew two\n", timeout=10, what="a.txt (open, clean) saved with the replacements")
    crlf = (repo / "src" / "crlf.txt").read_bytes()
    if crlf != b"new\r\nkeep\r\n":
        raise Failed(f"crlf.txt is {crlf!r}")
    if (repo / "src" / "main.rs").read_text() != "fn main() {\n    let old = 1;\n}\n":
        raise Failed("the dirty open main.rs was written to disk")
    if "let new = 1" not in stet.text():
        raise Failed("the dirty open main.rs did not get the replacement in its buffer")
    if (repo / "target" / "ignored.txt").read_text() != "old ignored\n":
        raise Failed("a .gitignore'd file was changed")
    diff = git(repo, "diff", "--stat").stdout.strip()
    return f"git diff --stat: {diff.splitlines()[-1] if diff else 'nothing'}; dirty main.rs replaced in its buffer only; CRLF kept"


def menu_items_of(stet, roles):
    found = []

    def walk(node, depth=0):
        for child in children(node):
            if alive(lambda n: n.get_role_name() in roles
                     and n.get_state_set().contains(Atspi.StateType.SHOWING), child):
                found.append(child)
            if depth < 40:
                walk(child, depth + 1)
    walk(stet.accessible())
    return found


INTERACTIVE = ("push button", "button", "toggle button", "check box", "radio button", "entry",
               "text", "combo box", "menu item", "check menu item", "radio menu item", "page tab",
               "list item", "spin button", "slider", "link")


def unnamed_widgets(stet, where):
    """Showing interactive widgets without an accessible name, as Orca would meet them."""
    found = []

    def walk(node, depth=0):
        for child in children(node):
            try:
                role = child.get_role_name()
                states = child.get_state_set()
                name = (child.get_name() or "").strip()
                shown = (role in INTERACTIVE and states.contains(Atspi.StateType.SHOWING)
                         and states.contains(Atspi.StateType.SENSITIVE))
                if shown and not name:
                    text = ""
                    if role in ("text", "entry"):
                        text = Atspi.Text.get_text(child, 0, 30)
                    found.append(f"{where}: {role}" + (f" holding {text!r}" if text else ""))
                elif shown and re.search(r"(^|\s)_[^\s_]", name):
                    found.append(f"{where}: {role} named {name!r} (a mnemonic underscore)")
            except GLib.Error:
                pass
            if depth < 40:
                walk(child, depth + 1)
    walk(stet.accessible())
    return found


@check
def check_a11y_audit(ctx):
    """M9: every showing interactive widget (editor, find bar's three rows, palette, the
    status bar's popovers, a dialog, the tab menu) has an accessible name for Orca. The editor
    itself is exempt (a text view with the document is announced by its role and text)."""
    stet = ctx.track(Stet(ctx.file("audit.txt", "audit me\n")))
    problems = []
    def take(where):
        problems.extend(p for p in unnamed_widgets(stet, where)
                        if not p.endswith("holding 'audit me\\n'") and "holding 'audit me" not in p)
    take("window")
    keys("ctrl+h")
    time.sleep(0.5)
    take("find bar (Ctrl+H)")
    keys("ctrl+shift+f")
    time.sleep(0.5)
    take("find in files row")
    keys("Escape")
    keys("ctrl+shift+p")
    time.sleep(0.5)
    take("palette")
    keys("Escape")
    time.sleep(0.3)
    open_tab_menu(stet, "audit.txt")
    take("tab menu")
    keys("Escape")
    time.sleep(0.3)
    keys("alt+c")
    time.sleep(0.6)
    take("Column Editor")
    keys("Escape")
    unique = sorted(set(problems))
    if unique:
        raise Failed(f"{len(unique)} kinds of unnamed widgets: " + "; ".join(unique))
    return "every showing interactive widget has an accessible name"


def line_offset(text, line):
    """The character offset of the start of 1-based `line`."""
    return sum(len(l) + 1 for l in text.split("\n")[:line - 1])


def caret_line(stet):
    match = re.match(r"Ln (\d+),", stet.caret())
    return int(match.group(1)) if match else None


@check
def check_bookmarks(ctx):
    """M7: Ctrl+Alt+K toggles a bookmark, Ctrl+Alt+L and Ctrl+Alt+J go to the next and previous
    one, and a click in the gutter with the real mouse toggles one too."""
    text = "".join(f"line {n}\n" for n in range(1, 31))
    stet = ctx.track(Stet(ctx.file("marks.txt", text)))
    editor = stet.editor()
    for line in (5, 20):
        caret_to(stet, line_offset(text, line))
        wait_for(lambda: caret_line(stet) == line, what=f"the caret on line {line}")
        keys("ctrl+alt+k")
    time.sleep(0.8)
    box = Atspi.Text.get_character_extents(editor, line_offset(text, 10), Atspi.CoordType.WINDOW)
    view = editor.get_extents(Atspi.CoordType.WINDOW)
    client = stet.client()
    # The bookmark gutter is the strip between the line numbers and the text; a click on the
    # line numbers only moves the caret, as GtkSourceView does.
    pointer("click", client["at"][0] + box.x - 15, client["at"][1] + box.y + box.height // 2)
    time.sleep(0.4)
    caret_to(stet, 0)
    seen = []
    for _ in range(3):
        before = caret_line(stet)
        keys("ctrl+alt+l")
        wait_for(lambda: caret_line(stet) != before, what="Ctrl+Alt+L to the next bookmark")
        seen.append(caret_line(stet))
    keys("ctrl+alt+j")
    wait_for(lambda: caret_line(stet) == 10, what="Ctrl+Alt+J back to the bookmark on line 10")
    if seen != [5, 10, 20]:
        raise Failed(f"Ctrl+Alt+L visited lines {seen}, not 5, 10 (the gutter click) and 20")
    return "Ctrl+Alt+K on lines 5 and 20 and a gutter click on line 10; Ctrl+Alt+L visited 5, 10, 20; Ctrl+Alt+J went back"


@check
def check_altgr_digits(ctx):
    """M7's Ctrl+1–5 and Ctrl+Alt+1–5 keys leave AltGr alone: on the Norwegian layout AltGr+2–5
    and 7–0 still type @ £ $ ½ { [ ] } into the document."""
    stet = ctx.track(Stet(ctx.file("altgr.txt", "")))
    wait_for(lambda: active()[0] == APP_ID, what="focus on Stet")
    require_active(APP_ID)
    for digit in "23457890":
        press("MOD5", digit)
    wanted = "@£$½{[]}"
    wait_for(lambda: stet.text() == wanted, what=f"AltGr+digits to type {wanted}")
    return f"AltGr+2, 3, 4, 5, 7, 8, 9, 0 typed {wanted}"


@check
def check_split_view(ctx):
    """M7: Move to Other View makes a second tab strip; a real click on a tab there gives it the
    focus and the title; a clone shows the same document, so typing in one shows in the other."""
    a = ctx.file("left.txt", "left side\n")
    b = ctx.file("right.txt", "right side\n")
    stet = ctx.track(Stet(a, b))
    wait_for(lambda: "right.txt" in stet.title(), what="right.txt in front")
    keys("ctrl+shift+p")
    time.sleep(0.4)
    type_text("Move to Other View")
    keys("Return")
    strips = wait_for(lambda: (lambda tabs: tabs if len(tabs) >= 2 else None)(
        [n for n in menu_items_of(stet, ("page tab",))]), what="two tab strips")
    left = next(n for n in strips if "left.txt" in (n.get_name() or ""))
    time.sleep(0.6)
    pointer("click", *screen_centre(stet, left))
    wait_for(lambda: "left.txt" in stet.title(), what="a click on left.txt in the first strip")
    keys("ctrl+shift+p")
    time.sleep(0.4)
    type_text("Clone to Other View")
    keys("Return")
    def views():
        return [n for n in menu_items_of(stet, ("text",))
                if alive(lambda m: Atspi.Text.get_text(m, 0, -1).startswith("left side"), n)]
    pair = wait_for(lambda: (lambda v: v if len(v) >= 2 else None)(views()), what="left.txt in both views")
    caret_to(stet, 0)
    type_text("X")
    wait_for(lambda: all(Atspi.Text.get_text(v, 0, -1).startswith("Xleft side") for v in views()),
             what="the typing in both views of the clone")
    return "Move to Other View made two strips; a click on left.txt took the focus and title; a clone showed the typing in both views"


@check
def check_compare(ctx):
    """M7: Ctrl+Alt+C compares the current document with the previous one, side by side;
    Alt+PgDn walks the differences; Ctrl+Alt+X clears. A screenshot is kept for the eye."""
    old = "".join(f"line {n}\n" for n in range(1, 41))
    new_lines = [f"line {n}\n" for n in range(1, 41)]
    new_lines[4] = "line 5 changed\n"
    del new_lines[19]
    new_lines.insert(30, "an added line\n")
    a = ctx.file("old.txt", old)
    b = ctx.file("new.txt", "".join(new_lines))
    stet = ctx.track(Stet(a, b))
    wait_for(lambda: "new.txt" in stet.title(), what="new.txt in front")
    keys("ctrl+alt+c")
    summary = wait_for(lambda: next((n for n in visible_names(stet) if "added" in n or "removed" in n
                                     or "changed" in n), None), timeout=10, what="the comparison's summary")
    first = caret_line(stet)
    keys("alt+Next")
    wait_for(lambda: caret_line(stet) != first, what="Alt+PgDn to the next difference")
    second = caret_line(stet)
    shot = ctx.root / "compare.png"
    client = stet.client()
    subprocess.run(["grim", "-g", f"{client['at'][0]},{client['at'][1]} {client['size'][0]}x{client['size'][1]}",
                    str(shot)], check=True, timeout=10)
    keep = ROOT / ".local" / "live-compare.png"
    keep.parent.mkdir(exist_ok=True)
    shutil.copy(shot, keep)
    keys("ctrl+alt+x")
    wait_for(lambda: not any("added" in n or "removed" in n for n in visible_names(stet)
                             if n == summary), timeout=5, what="Ctrl+Alt+X to clear the comparison")
    return f"summary {summary!r}; first difference on line {first}, Alt+PgDn to line {second}; screenshot in .local/live-compare.png"


@check
def check_mark_large(ctx):
    """M7 on a 200,000-line log: Mark with Bookmark line, Remove Non-Bookmarked Lines, then one
    Ctrl+Z gives the whole log back."""
    lines = [f"2026-10-01 12:00:{n % 60:02} {'ERROR' if n % 10 == 0 else 'INFO'} request {n}\n"
             for n in range(200_000)]
    path = ctx.file("big.log", "".join(lines))
    total = sum(len(l) for l in lines)
    stet = ctx.track(Stet(path))
    editor = stet.editor()
    wait_for(lambda: Atspi.Text.get_character_count(editor) >= total, timeout=30, what="the whole log")
    keys("ctrl+m")
    wait_for(lambda: stet.showing("push button", "Mark All"), what="the Mark row")
    keys("ctrl+a")
    type_text("ERROR")
    box = wait_for(lambda: stet.find(lambda n: n.get_role_name() == "check box"
                                     and (n.get_name() or "") == "Bookmark line"
                                     and n.get_state_set().contains(Atspi.StateType.SHOWING)),
                   what="the Bookmark line box")
    if not box.get_state_set().contains(Atspi.StateType.CHECKED):
        pointer("click", *screen_centre(stet, box))
        wait_for(lambda: box.get_state_set().contains(Atspi.StateType.CHECKED), what="Bookmark line on")
    Atspi.Action.do_action(stet.showing("push button", "Mark All"), 0)
    wait_for(lambda: shows(stet, "20,000") or shows(stet, "20000"), timeout=20, what="20,000 marks")
    keys("Escape")
    started = time.monotonic()
    keys("ctrl+shift+p")
    time.sleep(0.4)
    type_text("Remove Non-Bookmarked Lines")
    keys("Return")
    kept = sum(len(l) for l in lines if "ERROR" in l)
    wait_for(lambda: Atspi.Text.get_character_count(stet.editor()) == kept, timeout=30,
             what="only the ERROR lines left")
    removed_s = time.monotonic() - started
    keys("ctrl+z")
    wait_for(lambda: Atspi.Text.get_character_count(stet.editor()) == total, timeout=30,
             what="one Ctrl+Z to give the whole log back")
    return (f"Mark marked 20,000 ERROR lines and bookmarked them; Remove Non-Bookmarked Lines left "
            f"{kept:,} characters ({removed_s:.1f} s with the palette); one Ctrl+Z restored {total:,}")


def visible_names(stet):
    """Names of every visible label, button and notification: banners, toasts, status bar."""
    names = []

    def walk(node, depth=0):
        if node is None or depth > 40:
            return
        if alive(lambda n: n.get_state_set().contains(Atspi.StateType.SHOWING) and n.get_role_name() in (
                "label", "push button", "toggle button", "notification", "alert", "info bar"), node):
            names.append(alive(lambda n: n.get_name() or "", node) or "")
        for child in children(node):
            walk(child, depth + 1)
    walk(stet.accessible())
    return names


def shows(stet, needle):
    return any(needle.lower() in name.lower() for name in visible_names(stet))


@check
def check_external_reload(ctx):
    """A clean document reloads when another program rewrites the file, without taking focus."""
    path = ctx.file("external.txt", "before\n")
    stet = ctx.track(Stet(path))
    focus(ctx.original)
    wait_for(lambda: active()[0] != APP_ID, what="focus to move away")
    path.write_text("after\n")
    wait_for(lambda: stet.text() == "after\n", what="the reload")
    renamed = path.with_name(".external.txt.new")
    renamed.write_text("replaced by rename\n")
    os.replace(renamed, path)
    wait_for(lambda: stet.text() == "replaced by rename\n", what="the reload after a rename-over save")
    if active()[0] == APP_ID:
        raise Failed("Stet took the focus while reloading")
    return "rewrite and rename-over both reloaded in the background; focus stayed put"


@check
def check_dirty_external_change(ctx):
    """With unsaved edits, an outside change shows the Reload / Keep Mine banner."""
    path = ctx.file("dirty.txt", "base\n")
    stet = ctx.track(Stet(path))
    wait_for(lambda: active()[0] == APP_ID, what="focus on Stet")
    type_text("x")
    wait_for(lambda: stet.text() == "xbase\n", what="the typed edit")
    path.write_text("changed elsewhere\n")
    wait_for(lambda: shows(stet, "changed on disk"), what="the changed-on-disk banner")
    stet.press_button("Keep Mine")
    if stet.text() != "xbase\n":
        raise Failed(f"Keep Mine changed the text to {stet.text()!r}")
    return "banner shown; Keep Mine kept the unsaved text"


@check
def check_read_only_banner(ctx):
    """A file the user can't write shows the read-only banner with the sudoedit command."""
    path = ctx.file("readonly.txt", "locked\n")
    path.chmod(0o444)
    stet = ctx.track(Stet(path))
    wait_for(lambda: shows(stet, "sudoedit"), what="the read-only banner with sudoedit")
    return "read-only banner with the sudoedit command"


@check
def check_encodings(ctx):
    """Files in legacy encodings decode correctly and the status bar names the encoding."""
    cases = [("sjis.txt", "日本語のテキスト\n".encode("shift_jis"), "日本語のテキスト\n", "Shift_JIS"),
             ("utf16.txt", "\ufeffüñí ✓\n".encode("utf-16-le"), "üñí ✓\n", "UTF-16 LE"),
             ("cp1252.txt", "café\r\n".encode("cp1252"), "café\n", "1252")]
    seen = []
    for name, data, text, label in cases:
        path = ctx.root / name
        path.write_bytes(data)
        stet = ctx.track(Stet(path))
        wait_for(lambda: stet.text() == text, what=f"the decoded text of {name}")
        wait_for(lambda: shows(stet, label), what=f"the {label} label for {name}")
        if name == "cp1252.txt" and not shows(stet, "CRLF"):
            raise Failed("the CRLF file does not show CRLF")
        seen.append(f"{name}: {label}")
        stet.stop()
    return "; ".join(seen)


@check
def check_large_file(ctx):
    """A 94 MiB file shows its first screen fast, keeps loading, and opens in large-file mode."""
    big = ROOT / "target/fixtures/big-1m-lines.txt"
    if not big.exists():
        raise Skipped("run python3 tools/gen-fixtures.py first")
    expected = len(big.read_text())
    started = time.monotonic()
    stet = ctx.track(Stet(big))
    wait_for(lambda: Atspi.Text.get_character_count(stet.editor()) > 0, timeout=10, interval=0.01,
             what="the first screen")
    readable = (time.monotonic() - started) * 1000
    wait_for(lambda: Atspi.Text.get_character_count(stet.editor()) >= expected, timeout=60,
             interval=0.1, what="the whole file")
    full = time.monotonic() - started
    wait_for(lambda: shows(stet, "Large file"), what="the large-file banner")
    # Stet logs the first frame painted with text, from its process start; reading the text over
    # AT-SPI adds round trips to a main loop that is busy taking the rest of the file in.
    line = next(l for l in stet.lines if "first-frame" in l)
    first = float(line.rsplit(" ", 1)[-1])
    if first > 1500:
        raise Failed(f"first screen after {first:.0f} ms (> 1.5 s)")
    return (f"first screen {first:.0f} ms after the process started (text readable over AT-SPI"
            f" at {readable:.0f} ms), fully loaded {full:.2f} s, large-file banner shown")


@check
def check_typing_after_open(ctx):
    """Typing right after opening a 10 MB Rust file shows up at once (ADR-014, no stall)."""
    medium = ROOT / "target/fixtures/medium-10mb.rs"
    if not medium.exists():
        raise Skipped("run python3 tools/gen-fixtures.py first")
    stet = ctx.track(Stet(medium))
    wait_for(lambda: Atspi.Text.get_character_count(stet.editor()) > 0, timeout=10, what="the first screen")
    wait_for(lambda: active()[0] == APP_ID, what="focus on Stet")
    # One key at a time. A send_key_state key press takes ~200 ms of hyprctl round trips and
    # pauses, so each key is timed from the end of its injection until AT-SPI shows it: a stall
    # in Stet shows up there, while the injection speed does not.
    typed = "abcdefghijklmnopqrst"
    editor = stet.editor()
    latencies = []
    for i, char in enumerate(typed, 1):
        type_text(char)
        injected = time.monotonic()
        wait_for(lambda: Atspi.Text.get_text(editor, 0, i) == typed[:i], timeout=5, interval=0.002,
                 what=f"typed character {i}")
        latencies.append((time.monotonic() - injected) * 1000)
    worst = max(latencies)
    if worst > 100:
        raise Failed(f"a key was still missing {worst:.0f} ms after its injection"
                     f" (latencies {[round(l) for l in latencies]})")
    return (f"20 keys right after opening the 10 MB file were each visible within {worst:.0f} ms"
            f" of their injection finishing (median {statistics.median(latencies):.0f} ms)")


@check
def check_search_results(ctx):
    """Ctrl+H shows the replace row; Find All fills the results panel; Ctrl+Alt+Down/Up walk it."""
    stet = ctx.track(Stet(ctx.file("results.txt", "alpha beta\ngamma\nbeta delta\nbeta\n")))
    keys("ctrl+f")
    fields = wait_for(lambda: stet.count_showing("entry", "text"), what="the find row")
    keys("ctrl+h")
    wait_for(lambda: stet.count_showing("entry", "text") > fields, what="Ctrl+H to add the replace row")
    keys("ctrl+f")
    type_text("beta")
    keys("Escape")
    keys("ctrl+shift+p")
    type_text("Find All in Current Document")
    keys("Return")
    wait_for(lambda: shows(stet, "3 hits") or shows(stet, "3 matches") or shows(stet, "Line 3:"),
             what="Find All results in the panel")
    keys("ctrl+Home")
    keys("ctrl+alt+Down")
    first = wait_for(lambda: (lambda c: c if c.startswith("Ln ") else None)(stet.caret()), what="a result")
    keys("ctrl+alt+Down")
    wait_for(lambda: stet.caret() != first, what="Ctrl+Alt+Down to the next result")
    keys("ctrl+alt+Up")
    wait_for(lambda: stet.caret().split(",")[0] == first.split(",")[0], what="Ctrl+Alt+Up back")
    return f"replace row, Find All panel and result navigation ({first.split(',')[0]} and back)"


@check
def check_live_selftests(ctx):
    """Every CI self-test script also passes on the live Wayland display.

    Each script's window is focused as soon as it appears: Wayland refuses popovers (the palette,
    pickers) that a script opens in a window without keyboard focus."""
    summary = []
    for script in sorted((ROOT / "tests/selftest").glob("*.stet-test")):
        if script.stem.startswith(("preview", "perf-")):
            continue
        env = dict(os.environ, STET_APP_ID=APP_ID, STET_SELFTEST_ALLOW_LIVE="1")
        log = ctx.root / f"{script.stem}.log"
        with open(log, "w") as out:
            proc = subprocess.Popen([str(EXE), "--self-test", str(script)], env=env, cwd=ROOT,
                                    stdout=out, stderr=subprocess.STDOUT, text=True)
            try:
                client = wait_for(lambda: next((c for c in hypr("clients")
                                                if c.get("pid") == proc.pid), None),
                                  timeout=15, what=f"the {script.stem} self-test window")
                focus(client["address"])
                proc.wait(timeout=180)
            except (Failed, subprocess.TimeoutExpired) as error:
                proc.kill()
                proc.wait()
                lines = log.read_text().splitlines()
                last = next((l for l in reversed(lines) if l.startswith(("ok ", "not ok"))), "")
                raise Failed(f"{script.stem}: {error}; last step: {last}")
        lines = log.read_text().splitlines()
        last = (lines or [""])[-1]
        if proc.returncode != 0:
            failures = [l for l in lines if l.startswith("not ok")][:3]
            raise Failed(f"{script.stem}: {last} {failures}")
        summary.append(f"{script.stem} {last.removeprefix('# ').split(' of ')[0]}")
    return "; ".join(summary)


# ---------------------------------------------------------------------------------- runner

class Context:
    def __init__(self, root, original):
        self.root = root
        self.original = original
        self.stets = []
        self.processes = []
        self.scratch = []

    def file(self, name, text):
        path = self.root / name
        path.write_text(text)
        return path

    def dir(self, name):
        path = self.root / name
        path.mkdir()
        return path

    def track(self, stet):
        self.stets.append(stet)
        return stet

    def home_dir(self):
        """A scratch folder on the home directory's filesystem (under ~/.cache), removed when
        the check ends: GLib trashes a file into $XDG_DATA_HOME/Trash only when it is on the
        same filesystem as $HOME, and refuses to make a trash folder on /tmp."""
        path = Path(tempfile.mkdtemp(prefix="stet-live-", dir=Path.home() / ".cache"))
        self.scratch.append(path)
        return path

    def track_process(self, process):
        self.processes.append(process)
        return process

    def cleanup(self):
        for stet in self.stets:
            stet.stop()
        for process in self.processes:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    process.kill()
        self.stets.clear()
        self.processes.clear()
        for path in self.scratch:
            shutil.rmtree(path, ignore_errors=True)
        self.scratch.clear()


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--build", action="store_true", help="cargo build --release first")
    parser.add_argument("--only", help="comma-separated check names")
    parser.add_argument("--list", action="store_true", help="list the checks and exit")
    args = parser.parse_args()
    if args.list:
        for name, function in CHECKS.items():
            print(f"{name:20} {function.__doc__}")
        return 0
    if args.build:
        subprocess.run(["cargo", "build", "--release", "-p", "stet"], cwd=ROOT, check=True)
        subprocess.run(["cargo", "build", "--release"], cwd=POINTER.parents[2], check=True)
    # live_selftests is opt-in: the self-test harness uses its own app-id and asserts colours of
    # the headless built-in palette, so it is a CI tool rather than a live acceptance check.
    selected = args.only.split(",") if args.only else [c for c in CHECKS if c not in OPT_IN]
    original_class, original = active()
    saved_clipboard = clipboard()
    results = []
    with tempfile.TemporaryDirectory(prefix="stet-live-") as tmp:
        for name in selected:
            ctx = Context(Path(tmp) / name, original)
            ctx.root.mkdir()
            SANDBOX.clear()
            SANDBOX.update(XDG_STATE_HOME=str(ctx.dir("xdg-state")),
                           XDG_CACHE_HOME=str(ctx.dir("xdg-cache")))
            started = time.monotonic()
            try:
                detail = CHECKS[name](ctx)
                status = "PASS"
            except Skipped as skip:
                status, detail = "SKIP", str(skip)
            except Failed as failure:
                status, detail = "FAIL", str(failure)
            except Exception as error:  # noqa: BLE001
                status, detail = "FAIL", f"{type(error).__name__}: {error}"
            finally:
                ctx.cleanup()
                try:
                    focus(original)
                except Failed:
                    pass
            elapsed = time.monotonic() - started
            results.append((name, status, detail, elapsed))
            print(f"{status:4} {name:20} {elapsed:5.1f} s  {detail}", flush=True)
            time.sleep(0.5)
    if saved_clipboard is not None:
        # wl-copy keeps serving the clipboard in the background; detach it from our pipes.
        subprocess.Popen(["wl-copy", "--", saved_clipboard], stdin=subprocess.DEVNULL,
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    failed = [r for r in results if r[1] == "FAIL"]
    print(f"\n{len(results) - len(failed)} of {len(results)} checks passed "
          f"(focus restored to {original_class}).")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
