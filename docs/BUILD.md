# Building and verifying Stet

Project: Stet, a fast, keyboard-first text and code editor for Omarchy

Synced: 2026-11-13

Run commands from the repository root (the original working copy is `/home/fredrick/Projects/linux-apps/notes`; the folder keeps the M0 working name). Milestones M1, M3 and M4 are built and merged (Wave A, 2026-10-01): the editor shell with its headless self-tests and a local Arch package, the file pipeline for encodings and large files, and search; M2 and M5 are next. [PLAN.md](PLAN.md) has the current state.

## Toolchain

- Rust edition 2024, Cargo resolver 3.
- **MSRV 1.92** (`rust-version` in the workspace manifest, the same floor as Hertz). The development machine has **rustc and cargo 1.98.1**; `cargo +1.92 check --workspace --all-targets --locked` passed on 2026-11-13, and the CI workflow (`.github/workflows/ci.yml`) checks the floor on every push.
- rustfmt and Clippy are required. Arch's `rust` package includes them; rustup users add the `rustfmt` and `clippy` components and should not also install Arch's `rust`.
- No C or C++ compiler is needed for the planned dependency set.

## System packages

On Arch/Omarchy:

```sh
sudo pacman -S --needed rust gtk4 libadwaita gtksourceview5 pcre2 pkgconf
```

Versions on the development machine (2026-09-30): gtk4 4.22.4, libadwaita 1.9.3, gtksourceview5 5.20.0, pcre2 10.48, glib2 2.88.3, fcitx5 5.1.22, Omarchy 4.0.4.

- Crate feature flags must not exceed the system libraries: `gtk4` with `v4_18`, `libadwaita` with `v1_9`, `sourceview5` with `v5_18` (the crate has no `v5_20` flag; `v5_22` waits until Arch ships GtkSourceView 5.22).
- Keep `pcre2` and `pkgconf` installed: if pkg-config can't find the system PCRE2, `pcre2-sys` silently builds its bundled copy.
- Headless runs use `gtk4-broadwayd` (part of `gtk4`) and, for D-Bus tests, `dbus-run-session` (part of `dbus`). The tools need Python 3.

## Workspace layout

```
notes/               repository root (this checkout's folder keeps the M0 working name)
  Cargo.toml         workspace: edition 2024, resolver 3, default-members = ["app"], publish = false
  app/               crate `stet` (binary): the only production crate that links GTK
  domain/            crate `stet-domain`: pure logic, one module per bounded context (ADR-020): text,
                     document, search, ops, column, diff, marks, nav, fuzzy, session, settings,
                     actions, view, theme, updates; `domain/src/lib.rs` says what each holds
  infrastructure/    crate `stet-infrastructure`: std I/O
  assets/            brand/ (SVG icon masters), icons/ (PNG exports); provenance in docs/ASSETS.md
  packaging/         PKGBUILD.in, the desktop file, the metainfo, dependency-licenses.json and licenses/;
                     flatpak/ (the Flatpak's manifest and cargo-sources.json, 1.6)
  tests/selftest/    `.stet-test` scripts for `stet --self-test`, and their fixture themes
  tools/             ci.sh, headless.sh + headless-dbus.conf, gen-fixtures.py, package-source.py,
                     license-report.py, export-icons.sh, progress/ (preview screenshots)
  docs/              project memory; spike results in docs/spikes/, upstream issue drafts in docs/upstream/
  tests/fixtures/    small committed fixtures (encodings and line endings, from M3)
```

The app crate (`app/src/`): the top level is the application shell, `editor/` one document and its views, `window/` the window's features in groups, and `sys/` the only unsafe code ([ADR-020](DECISIONS.md#adr-020--code-architecture-layers-bounded-contexts-and-lints-2026-11-13)):

```
main.rs cli.rs application.rs activation.rs config.rs logging.rs monitor.rs preferences.rs actions.rs style.css
runtime/         the GTK thread's async plumbing: worker jobs, Latest tokens, debounce, Notify, one-shot signals
theme/           the live Omarchy theme and font: the generated scheme, the CSS, the fixtures
sys/             safe wrappers over the GTK and GtkSourceView C calls the bindings lack, the only module
                 allowed `unsafe`: search.rs (async Find Next/Previous), source_marks.rs (next source mark),
                 text_buffer.rs (an `insert-text` handler that may take over), accessible_text.rs
                 (NULL-safe GtkAccessibleText.get_selection)
editor/          one tab and its document: page.rs (EditorPage), document.rs, state.rs (DocState),
                 load.rs (streaming a text in), edits.rs (domain edits on the buffer), search.rs (search
                 contexts, our own matches, the current match), marks.rs (bookmarks and marks),
                 comparison.rs (one side of a comparison), banners.rs (the banner widget and the stack),
                 caret.rs, appearance.rs, session.rs (the tab's session record), tabstops.rs (ADR-015)
  column/        the editor's view, StetView, with column mode (ADR-016): view.rs, edit.rs, keys.rs,
                 paint.rs, clipboard.rs, dialog.rs (the Column Editor)
window/          mod.rs (Window, layout, lifecycle), dispatch.rs (every action to its feature), probe.rs
                 (the self-test's view), snapshot.rs (jobs on a text snapshot), dialogs.rs, focus.rs
  documents/     files.rs (open, save), disk.rs (outside changes), encoding.rs, banners.rs, session/
                 (restore.rs, stubs.rs, backup.rs, quit.rs, wait.rs)
  workspace/     tabs.rs (the tab menu's commands), pins.rs, split.rs, view.rs (the View menu), nav.rs
                 (Ctrl+Tab, back and forward), quick_open.rs
  text/          editing.rs (undo, clipboard, column commands), toolbox.rs, tool_dialogs.rs
  search/        bar/ (the bar, build.rs its widgets, history.rs), find.rs, replace.rs, results/
                 (the panel, commands.rs the window's side), files/ (Find and Replace in Files),
                 marking.rs (bookmarks, Mark, style tokens), smart.rs (smart highlighting)
  tools/         compare.rs, settings.rs (what config.toml and keys.toml do), updates.rs, default_editor.rs
  chrome/        status.rs, language.rs, encoding_picker.rs, indentation.rs, palette.rs, a11y.rs
selftest/        the `stet --self-test` harness (the `selftest` feature)
```

As built in M1 (2026-10-01):

```
app/             main.rs (local CLI parse, then the app or the self-test); cli.rs (clap); config.rs (app-id,
                 directories, test hooks); application.rs (GApplication, command line, first frame);
                 actions.rs (GActions, accelerators and the hamburger menu from the registry);
                 window/ (mod.rs tabs, files, closing, palette items; find.rs; palette.rs; language.rs;
                 status.rs; dialogs.rs); editor.rs (EditorPage: buffer, view, banner, file state);
                 theme.rs (palette, scheme, CSS, font, watches); monitor.rs (directory watch with the
                 parent); tabstops.rs (ADR-015); search.rs (async Find Next wrapper); worker.rs
                 (worker threads and async-channel); selftest/ (harness, script parser, screenshots);
                 style.css (square chrome)
domain/          actions.rs (the registry and its keymap tests); document.rs (names and titles);
                 language.rs (detection hints); location.rs (file:line:col, go to line); palette.rs
                 (matching); recent.rs; view.rs (zoom, font CSS, tab width, undo cap); text/; theme.rs
infrastructure/  text_file.rs (UTF-8 load and atomic save, until M3); recent.rs (recent.json);
                 xdg.rs (Stet's directories); omarchy/ (paths, colors, font); logging.rs
```

Added in M3 (2026-10-01); `infrastructure/src/text_file.rs` is gone, and every file goes through `infrastructure::fs`:

```
app/             loader.rs (worker side of opening: read and decode, the longest line, format or break
                 long lines, stream ~4 MiB pieces); banner.rs (banners on AdwBanner's CSS nodes, with any
                 number of buttons); editor.rs (the document model DocState, banners, loading by pieces,
                 large-file mode, the document's language); monitor.rs (+ FileWatch for open files);
                 window/ (files.rs open, load and save; disk.rs outside changes and reloads; encoding.rs
                 Reinterpret, Convert and line endings; encoding_picker.rs the status bar's picker;
                 banners.rs what each state shows); selftest/ (+ broadway.rs web client for timings,
                 generate.rs large inputs)
domain/          text/lines.rs (longest line, display breaks, line count); text/edit.rs (+ minimal_edit
                 for reloads); view.rs (+ the ADR-014 limits); document.rs (+ sudoedit_command)
infrastructure/  fs/ (+ remove_stale_temps; Fingerprint is the session's, ADR-006 amendment);
                 encoding/table.rs (+ entry ids and status-bar labels)
```

`s1_perf` has four more groups for the ADR-014 follow-up: `threshold`, `threshold-more`, `threshold-lines` and `threshold-untagged` ([S1 addendum](spikes/S1-performance.md#addendum-the-adr-014-follow-up-m3-2026-10-01)); they need `python3 tools/gen-fixtures.py` first.

Planned module layout inside the crates (from the approved plan):

```
app/             application.rs (command line, --wait, signals, D-Bus activation); actions/ (ActionId -> GAction,
                 menu, palette, accelerators); window/ (tabs, results panel, status bar, banners, toasts, palette);
                 editor/ (EditorPage, buffer_bridge.rs, large_mode.rs, stub_page.rs, column.rs in M6);
                 search/; theme/; session/
domain/          text/ (eol, TextEdit, position); ops/ (lines, sort, case, whitespace, comment, prefix_number,
                 json_fmt, xml, column in M6); search/ (query, Extended unescape, Boost->PCRE2 translate, template);
                 document.rs, session.rs, language.rs, theme.rs, actions.rs, keymap.rs, config.rs
infrastructure/  encoding/ (detect, decode, encode); fs/ (load, safe save, fingerprint); session_store/ (manifest,
                 backup writer, restore, lock); files/ (walk, filters, job handle and cancel token, open-documents
                 index; find/ = Find in Files, replace/ = Replace in Files, project.rs = quick open's walk);
                 omarchy/ (paths, colors, font, default_editor); config/; logging
```

### Dependency rules

- `stet-domain`: no I/O, no GTK, no threads. Only pure crates (for example `serde`, `thiserror`, `memchr`, `unicode-segmentation`).
- `stet-infrastructure`: depends on `stet-domain`; std I/O and worker-safe code; no GTK; every test runs headless.
- `stet` (the app): depends on both; links `gtk4`, `libadwaita` and `sourceview5`. Nothing blocking runs on the GTK thread: jobs run on GLib's thread pool (`runtime::worker::run`, over `gio::spawn_blocking`) and their results come back on the main loop. `app/src/runtime/` also holds the tokens that drop stale results (`Latest`), the debouncer, notifications to wait on instead of polling (`Notify`, `wait_until`, `EditorPage::loaded`) and one-shot signal handlers; `window/snapshot.rs` runs a job on a snapshot of a document's text and checks the result against the page.
- The M0 spike crate `stet-spikes` was removed on 2026-11-13 ([ADR-020](DECISIONS.md#adr-020--code-architecture-layers-bounded-contexts-and-lints-2026-11-13)); its code is at the tag `m0-spikes` (see "Spikes (M0)" below).
- Deliberately **not** used: `syntect`/`two-face` (GtkSourceView highlights), `ropey` ([ADR-003](DECISIONS.md)), `fancy-regex` ([ADR-005](DECISIONS.md)), `notify` (GFileMonitor), `zbus` (GApplication), `ashpd` (portals are the GTK default), `toml_edit` (no Preferences GUI), relm4, tokio. `serde_json`'s `arbitrary_precision` is never enabled: it breaks untagged and flattened types across the whole binary; the JSON tools are token-level.

The planned dependency versions are in the approved plan and [RESEARCH.md](RESEARCH.md#4-core-crates); `Cargo.toml` is the source of truth for what is actually used. Added in M1: `clap` 4.6 (builder API, without the `color` feature) and `async-channel` 2.5 in the app, `gio` directly with `v2_80` (for `GApplicationCommandLine::print_literal`), and `serde` and `serde_json` in `stet-infrastructure` for `recent.json`. Added in M2: `glib-unix` 0.22 in the app, for the SIGTERM, SIGHUP and SIGINT handlers (gtk-rs moved `unix_signal_add_local` out of `glib` into that crate), and `serde_json` in the app, which the self-test uses to read `session.json`. Added in M5: `toml` 1.1 in `stet-domain`, with only its `std` and `parse` features (spanned tables, no serde or writing), for `config.toml` and `keys.toml` ([ADR-017](DECISIONS.md#adr-017--settings-in-configtoml-and-keystoml-2026-10-01)).

## Build and run

```sh
cargo build                 # debug build of the app (the default member)
cargo run -- <files>        # open files in a debug build
cargo build --release
```

Debug builds use the app-id `io.github.pixdevsapps.Stet.Devel`; release builds use `io.github.pixdevsapps.Stet`. A debug build therefore never forwards its files to an installed copy.

`cargo run` opens a window on the current display. **Agents never run it on the user's live Wayland session**; they use the headless wrapper below.

Logging goes to stderr through `tracing`, filtered by `RUST_LOG` (default `info`; for example `RUST_LOG=stet=debug`). The app logs `first-frame <ms>` after its first paint. Document contents are never logged.

Command line (M1): `stet [FILE[:LINE[:COL]]]... [-n LINE] [-c COL]`, plus `--help`, `--version` and `--self-test <script>`. `--self-test` is hidden from `--help` and exists only in builds with the `selftest` Cargo feature, which is on by default ([ADR-020](DECISIONS.md#adr-020--code-architecture-layers-bounded-contexts-and-lints-2026-11-13)); `cargo build -p stet --no-default-features` builds the app without the harness, its probes and `serde_json`. A second `stet` hands its command line to the running instance; the `:LINE[:COL]` suffix is split off only when the literal path does not exist.

M2 adds `--wait` (`-w`: return when the files given are closed; status 1 if one closed unsaved) and `--no-session` (when it starts Stet: no restore and no session kept), and accepts GApplication's `--gapplication-service`, which the D-Bus service file runs. When no Stet is running, a command line asks the session bus to start the service and then hands over its files ([ADR-011 amendment](DECISIONS.md#adr-011-amendment--d-bus-activation-and---wait-as-built-in-m2-2026-10-01)). A build without an installed service file, such as `cargo run` (app-id `….Devel`), becomes the editor itself, as before, except with `--wait`, where it starts `stet --gapplication-service` in the background and waits for its own tabs.

Environment variables:

| Variable | Effect |
| --- | --- |
| `STET_EXIT_AFTER_MS=<ms>` | Quit after that many milliseconds; headless smoke runs use it |
| `STET_OMARCHY_STATE_DIR=<dir>` | Use this directory instead of `~/.local/state/omarchy` (tests) |
| `STET_APP_ID=<id>` | Override the application id (test hook: the self-test's second instances, child self-tests and the activation test's Stet use it) |
| `STET_SELFTEST_ROOT=<dir>` | Give `--self-test` this scratch directory and keep it afterwards (test hook: a parent self-test runs child self-tests over one directory) |
| `STET_SELFTEST_ALLOW_LIVE=1` | Let `--self-test` run on a live display; otherwise it requires Broadway |
| `STET_SELFTEST_STEP_TIMEOUT=<seconds>` | How long one self-test step may run before it fails as timed out (default 300) |
| `RUST_LOG` | Log filter, default `info` |

## Checks

```sh
bash tools/ci.sh
```

The script runs `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, the same Clippy run for the app without its self-test harness (`cargo clippy -p stet --no-default-features --all-targets -- -D warnings`, which fails if anything outside the harness needs it), `cargo test --workspace`, `cargo doc --workspace --no-deps` with warnings as errors, and `cargo build --workspace` (plus the checks that the Flatpak's crate list matches `Cargo.lock` and that every relative link in the Markdown files resolves, `tools/check-links.py`). Run it before every change is proposed.

```sh
bash tools/ci.sh --ui
```

`--ui` adds a headless smoke run (the app starts on Broadway with `STET_EXIT_AFTER_MS=1500` and must log `first-frame`; log `.local/ci-smoke.log`), then runs every self-test script in `tests/selftest/` except the preview scripts (`preview*`) and the performance scripts (`perf-*`) (logs `.local/selftest-<name>.log`). The Broadway display is `$STET_CI_DISPLAY`, default 9; parallel runs must use different numbers. [TESTING.md](TESTING.md) describes what the tests and the self-test scripts cover.

A single self-test script:

```sh
cargo build
tools/headless.sh 20 target/debug/stet --self-test tests/selftest/theme.stet-test
```

The preview screenshots in `tools/progress/public/` come from `tests/selftest/preview.stet-test` (M1), `preview-m3.stet-test` (M3) and `preview-search.stet-test` (M4), which need Omarchy's installed themes.

## Headless GTK runs

```sh
tools/headless.sh <display> <cmd...>
```

The wrapper (as scaffolded on 2026-09-30):

- starts `gtk4-broadwayd` on display `:<display>` (TCP port 8080 + display, bound to 127.0.0.1) and refuses a display that is already in use;
- runs `<cmd...>` inside a private D-Bus session (`dbus-run-session` with `tools/headless-dbus.conf`, which has no service directories, so no portal, gvfs or AT-SPI service is activated);
- removes `WAYLAND_DISPLAY`, `DISPLAY` and `HYPRLAND_INSTANCE_SIGNATURE` from the environment and sets `GDK_BACKEND=broadway`, `GTK_A11Y=none`, `ADW_DISABLE_PORTAL=1`, `GSETTINGS_BACKEND=memory` and `GIO_USE_VFS=local`;
- since 2026-11-07, points `XDG_CONFIG_HOME` at an empty temporary folder, removed afterwards: GTK reads the user's `gtk-4.0/gtk.css` (Omarchy Theme Studio writes one that sets libadwaita's colours) and fontconfig their `fonts.conf`, so a run depended on the desktop's setup;
- kills the command after `STET_HEADLESS_TIMEOUT` seconds (default 300);
- logs the daemon to `.local/broadway-<display>.log` (the wrapper creates `.local/` if needed), stops it when the command exits, and removes the socket file it leaves behind.

**Why the private bus has its own configuration.** A plain `dbus-run-session` uses the standard session-bus configuration, which lets the bus start services on demand. On 2026-09-30, during M0, such a private bus auto-started xdg-desktop-portal-hyprland, and the portal connected to the live Hyprland session. `tools/headless-dbus.conf` fixed that: it lists no service directories, so the private bus cannot start any service. **Never bypass it**: do not run GTK programs under a bare `dbus-run-session`, and do not remove `--config-file` from the wrapper.

**D-Bus activation tests (M2)** need a bus that can start Stet, and nothing else. `activation.stet-test`'s `test-bus-start` runs a second private `dbus-daemon` inside the wrapper, with a configuration it writes into the run's scratch directory: its only `<servicedir>` is `$TMP/testbus/services`, which holds Stet's own test service file or nothing, and it listens on a socket in a directory of its own under the system temporary directory (socket paths are limited to 108 bytes). The activated Stet gets its own application id and XDG directories. `test-bus-stop` (and the end of the run) stops a Stet it started that still runs, then the daemon. Never give such a bus `<standard_session_servicedirs/>`.

Each agent or parallel run uses its own display number. Make the program end on its own as well, for example:

```sh
cargo build
tools/headless.sh 11 env STET_EXIT_AFTER_MS=3000 target/debug/stet
pgrep -a gtk4-broadwayd    # should print nothing once the run is over
```

With no browser connected, GDK Broadway paints at most about one frame per second (S1 measured 1.0 Hz), so any wall-clock paint or undo timing stretches to whole seconds. Time the GTK work inside a frame (`before-paint` to `after-paint`), or answer Broadway's frame acknowledgements as `s1_perf`'s `connect_web_client` does, and turn cursor blink off in timing runs.

Broadway is not Wayland. A headless run proves that the code builds, starts, renders and exits cleanly, and gives indicative timings. It sees neither the user's portal settings nor gsettings, and it does not cover the Hyprland compositor, fractional scaling, text-input-v3 and fcitx5, the clipboard between apps, drag-and-drop from Nautilus, or window rules. Those are checked in spike S4 and the manual acceptance list in the live session, by the user or with explicit authorization.

## Live acceptance script

`tools/live/acceptance.py` runs the release build in the live Hyprland session and drives it like a user would. It needs explicit authorization (it opens windows on the current workspace and types into them), and nobody should type while it runs.

```bash
cargo build --release
cargo build --release --manifest-path tools/live/pointer/Cargo.toml
python3 tools/gen-fixtures.py               # for large_file and typing_after_open
python3 tools/live/acceptance.py --list
python3 tools/live/acceptance.py            # all default checks, about a minute
python3 tools/live/acceptance.py --only shortcuts,encodings
```

- **Isolation:** the app-id is `io.github.pixdevsapps.Stet.LiveTest` (through `STET_APP_ID`), so the run never talks to a real Stet. Every check gets its own `XDG_STATE_HOME` and `XDG_CACHE_HOME` (sessions and backups never touch the user's, nor leak between checks) and a temporary directory for its files; `tab_menu` trashes into a sandboxed `XDG_DATA_HOME` under `~/.cache` (GLib trashes only on the home filesystem) that it removes. Theme and font checks use a sandboxed Omarchy state directory (`STET_OMARCHY_STATE_DIR`) and `XDG_CONFIG_HOME`; only `theme_refresh` runs the real `omarchy-theme-refresh`, which re-applies the current theme. `tab_menu` opens and closes the user's terminal and file manager.
- **Input:** keys go through Hyprland's Lua dispatcher `hl.dsp.send_key_state`, which resolves keys in the real keyboard's keymap, as Omarchy's own binds do. `wtype` is not used: its private keymap breaks GTK's matching of Shift accelerators, and after it has run, `send_key_state` cannot find keys. Each key press costs about 200 ms of `hyprctl` round trips, so time Stet from the end of an injection. The mouse uses `tools/live/pointer` (`zwlr_virtual_pointer_v1`), and drags come from `tools/live/drag_source.py`, a GTK 4 window offering a file list.
- **Reading state:** AT-SPI for the editor text, caret, focus and dialogs (`Atspi.Text.set_selection`, never `add_selection`, which crashes GTK 4.22: [upstream note](upstream/gtk-accessible-text-add-selection.md)); `hyprctl -j clients` for windows, focus, floating and full screen; `grim` for on-screen colours; Stet's log lines on stderr.
- **Afterwards:** focus goes back to the window that had it, the clipboard text is restored, and every process it started is stopped.
- **Keys on the Norwegian layout:** `send_key_state` finds only a key's first-level keysym, so characters such as `:` and `/` are typed as Shift with their key (`SHIFTED_KEYS`). It cannot hold a modifier for pointer events, so Alt+drag is checked as "Hyprland grabs no Alt mouse button" (`alt_drag_free`).
- **Opt-in checks:** `--only live_selftests` runs the self-test scripts in the live session (the harness uses its own app-id and asserts the headless built-in palette's colours); `--only dbus_service` installs a D-Bus service file for the test app-id in `~/.local/share/dbus-1/services/` for the check and removes it, with any folders it made and the systemd slice, afterwards.
- The pointer tool also does `double-click` (two clicks 120 ms apart, well inside GTK's 400 ms), `right-click`, `back` and `forward` (buttons 8 and 9); pointer actions first move next to the spot and let a new window finish its open animation.

## Fixtures

```sh
python3 tools/gen-fixtures.py
```

Writes the large generated inputs to `target/fixtures/`. They are **not committed** (`target/` is build output), and `cargo clean` removes them, as it did on 2026-09-30 when `target/` was cleaned to free disk space. Regenerate them with `python3 tools/gen-fixtures.py` before running a spike. Output is seeded, so every run produces identical files; existing files are kept unless `--force` is given. As scaffolded on 2026-09-30 it writes `big-1m-lines.txt` (1M lines of code-like text, for the open test), `medium-10mb.rs` (typing latency), `single-line-10mb.json` (the long-line path) and `search-1m.log` (replace-all). The script prints each file's size; record the actual sizes with spike results rather than assuming them. Small hand-made fixtures (encodings, line endings, NUL, binary) are committed under `tests/fixtures/` from M3.

## Spikes (M0)

The spike code left the tree on 2026-11-13 ([ADR-020](DECISIONS.md#adr-020--code-architecture-layers-bounded-contexts-and-lints-2026-11-13)); its results stay in `docs/spikes/`. To run a spike again, check out the tag in a worktree of its own and run the commands below there:

```sh
git worktree add ../stet-m0-spikes m0-spikes
```

Generate the fixtures, build once, then run each spike headless on your own display number. Full runs take far longer than the wrapper's default 300 s limit, so raise `STET_HEADLESS_TIMEOUT`. On 2026-09-30, S1 and S2 used up to 3000 and 3600 s, and a full S3 run took 7 min 43 s.

```sh
python3 tools/gen-fixtures.py
cargo build --release -p stet-spikes
STET_HEADLESS_TIMEOUT=3000 tools/headless.sh 11 cargo run -q --release -p stet-spikes --bin s1_perf -- all
STET_HEADLESS_TIMEOUT=300 tools/headless.sh 12 cargo run -q --release -p stet-spikes --bin s2_search -- --probes-only
STET_HEADLESS_TIMEOUT=1200 tools/headless.sh 13 cargo run --release -p stet-spikes --bin s3_theme
STET_HEADLESS_TIMEOUT=600 tools/headless.sh 15 cargo run --release -p stet-spikes --bin s5_column
```

| Binary | Arguments | Output |
| --- | --- | --- |
| `s1_perf` | Groups `open`, `typing`, `long-line`, `snapshot` or `all`, or case names; no argument runs every case. Each case runs 3 times in child processes. Under Broadway the binary connects its own web client; with `STET_SPIKE_ALLOW_LIVE=1` and no Broadway it runs on the live display. | A Markdown summary per case on stdout; raw runs in `.local/s1/` |
| `s2_search` | `--runs=N`, `--lines=N`, `--undo=…`, `--cases=dense,sparse`, `--no-redo`, `--replace-only`, `--probes-only`, `--perf-only`, `--tag=NAME` (see the binary's header). With no arguments it runs everything, including three view-attached undos of 1M replacements per run (about 2 minutes and 8 GiB each); run the measured sets from `S2-search.md` instead. | JSON lines on stdout; tables in `target/spikes/s2/` |
| `s3_theme` | none | JSON lines on stdout; tables in `.local/s3/results.md` |
| `s5_column` | optional output path | JSON lines on stdout; a table in `.local/s5-column.md` by default |
| `s4_live` | none (interactive), `--follow-up` (shorter instructions) or `--auto` (file dialog check, then prints `READY_CLIPBOARD` for a key driver). **Live display only**: run it with `STET_SPIKE_ALLOW_LIVE=1`, never through `tools/headless.sh`. | JSON results on Finish or close to `$STET_S4_OUT` (default `target/s4-live.json`); log lines on stdout |

The spike binaries refuse to start unless `GDK_BACKEND=broadway` (which `tools/headless.sh` sets); `STET_SPIKE_ALLOW_LIVE=1` overrides that, and is only for a live run the user has explicitly authorized. Keep the raw output under `.local/`. It is disposable: `.local/` and `target/` were deleted on 2026-09-30 to free disk space, so every number that matters goes into the report. The exact commands used for each recorded result are in its report; those of 2026-09-30 still use the working name (`notes-spikes`, `NOTES_*`).

Each result goes into its own file under `docs/spikes/` (`S1-performance.md`, `S2-search.md`, `S3-theme.md`, `S5-column.md`) with the date, exact command, machine and library versions, raw numbers, the pass bar, and caveats such as "measured under Broadway". [RESEARCH.md](RESEARCH.md#m0-spike-results) links them.

- **S6 startup** has no binary of its own. It times the release app headless, for example `tools/headless.sh 11 env STET_EXIT_AFTER_MS=800 target/release/stet`, and reads the `first-frame` log line. It is recorded in `S1-performance.md`. D-Bus-activated `--wait` needs M2 and the installed service file.
- **S4 (Wayland/Hyprland)** needs the live session, so it is not part of the headless set.

## Packaging

The Arch package is named `stet` (M1, adapted from Hertz):

```sh
python3 tools/license-report.py     # after dependency changes: packaging/dependency-licenses.json + licenses/
bash tools/export-icons.sh          # after icon changes: assets/icons/*.png from the SVG master
python3 tools/package-source.py     # .local/package/stet-<version>.tar.gz and PKGBUILD, from HEAD
cd .local/package && makepkg --cleanbuild --force
```

makepkg checks `makedepends` against pacman's database. Where Rust comes from rustup (as on the development machine), that check fails for `rust` and `cargo`; add `--nodeps` once the other dependencies are installed through pacman.

- `tools/package-source.py` archives the **committed** tree (`git archive HEAD`; it warns about uncommitted changes), gzips it with a zero timestamp, and fills `packaging/PKGBUILD.in`'s `@PKGVER@`, `@SHA256@` and `@SOURCE_DATE_EPOCH@` (the commit time).
- `PKGBUILD`: `prepare()` runs `cargo fetch --locked` for the host target; `build()` runs `cargo build --release --locked --offline`; `check()` runs only `cargo test --locked --offline -p stet-domain -p stet-infrastructure`, never the GTK self-tests (a clean build has no display).
- Depends: `gtk4 libadwaita gtksourceview5 glib2 pcre2 fontconfig curl gcc-libs glibc` (`curl` since 1.5, for Check for Updates); make depends: `rust cargo pkgconf`. `url` is the GitHub repository, `https://github.com/PixDevsApps/Stet` (public since 2026-10-02; the 1.0–1.3.0 packages were built without it, 1.3.1 and later have it).
- Installs `/usr/bin/stet`; `/usr/share/applications/io.github.pixdevsapps.Stet.desktop`; the AppStream metainfo `/usr/share/metainfo/io.github.pixdevsapps.Stet.metainfo.xml` (1.6); the D-Bus service file `/usr/share/dbus-1/services/io.github.pixdevsapps.Stet.service` (M2); the icon in hicolor (scalable, symbolic, and 16 to 256 px); `/usr/share/licenses/stet/` (LICENSE, THIRD_PARTY.md, `dependency-licenses.json` and each crate's license texts under `dependencies/`); README, INTEGRATIONS, INSTALL and USER_GUIDE under `/usr/share/doc/stet/`.
- `tools/license-report.py` follows the `stet` package's normal dependencies in the locked graph (dev- and build-dependencies are not shipped) and fails if a crate declares no license or ships no license text. Run it after every dependency change: on 2026-10-01 (M2) it had not run since M1 and listed 87 crates; after the merge of M2 and M5 it lists 130, M3's and M4's crates, M2's `glib-unix` and M5's `toml` included.
- The desktop file sets `DBusActivatable=true` and the service file runs `/usr/bin/stet --gapplication-service` (M2, [ADR-011 amendment](DECISIONS.md#adr-011-amendment--d-bus-activation-and---wait-as-built-in-m2-2026-10-01)).

### The Flatpak (1.6)

The Flatpak ([ADR-019](DECISIONS.md#adr-019--a-flatpak-from-stets-own-repository-2026-11-12)) needs `flatpak`, `flatpak-builder`, `appstreamcli`, `desktop-file-validate`, `ostree` and `gpg`, and from Flathub the GNOME 51 SDK and the Rust extension:

```sh
flatpak install --user flathub org.gnome.Sdk//51 org.gnome.Platform//51 org.freedesktop.Sdk.Extension.rust-stable//26.08
python3 tools/flatpak-cargo-sources.py      # after every Cargo.lock change; tools/ci.sh checks it
python3 tools/flatpak.py build              # HEAD, which must be the commit tagged v<version>
python3 tools/flatpak.py build --worktree   # a test: the files git would commit now
```

- `build` copies the tree into `.local/flatpak/release/<version>/src` (`git archive HEAD`, or with `--worktree` the tracked and new files), runs `flatpak-builder` with the manifest into `.local/flatpak/release/<version>/build` and an unsigned repository beside it, and checks the result: `appstreamcli validate`, a `<release>` for the version in the metainfo, `desktop-file-validate`, the exported D-Bus service rewritten into `flatpak run`, and no home or build path in `/app/bin/stet`.
- The build has no network: `flatpak-builder` downloads the crates `packaging/flatpak/cargo-sources.json` lists from static.crates.io beforehand, checked against `Cargo.lock`'s SHA-256, and Cargo reads them as a vendored source. `RUSTFLAGS` maps the build folder to `/usr/src/stet`.
- The Flatpak installs `/app/bin/stet`, the desktop entry, the metainfo, the D-Bus service (`Exec=/app/bin/stet --gapplication-service`), the icons, the license bundle in `/app/share/licenses/io.github.pixdevsapps.Stet/` and the four guides in `/app/share/doc/stet/`.
- `keygen` makes the repository's signing key once, in `.local/flatpak/gpg` (or `STET_FLATPAK_GPG_HOME`); `site` signs the build into `.local/flatpak/site/` (or `STET_FLATPAK_SITE`) with `stet.flatpakrepo`, `io.github.pixdevsapps.Stet.flatpakref` and an `index.html`, for `STET_FLATPAK_URL` (by default `https://pixdevsapps.github.io/stet-flatpak/`); `publish` force-pushes that folder as one commit to `PixDevsApps/stet-flatpak`, whose GitHub Pages serve it.
- To try a build without touching your own Flatpak installation, install it into a scratch one: `FLATPAK_USER_DIR=<scratch> flatpak --user …` ([TESTING.md](TESTING.md#2026-11-12--160-the-flatpak) shows how the 1.6.0 build was tried headless).

### Releasing

From 1.5.0 on, a GitHub release carries the tested package under one name only, `stet-x86_64.pkg.tar.zst` (`stet_domain::updates::PACKAGE`; the user's choice on 2026-11-12). The README's and [INSTALL.md](INSTALL.md#install-the-released-package)'s install line downloads it through `releases/latest/download/`, so it needs no change for a new release, and Check for Updates' command downloads it from the release's own tag. A release without that file breaks both; the release notes give the package's version and SHA-256:

```sh
cp .local/release/stet-<version>-1-x86_64.pkg.tar.zst .local/release/stet-x86_64.pkg.tar.zst
gh release create v<version> --repo PixDevsApps/Stet --verify-tag --title "Stet <version>" \
    --notes-file <notes> .local/release/stet-x86_64.pkg.tar.zst
```

From 1.6.0 on, a release also carries the Flatpak's `io.github.pixdevsapps.Stet.flatpakref` (from `tools/flatpak.py site`), which INSTALL.md's Flatpak command fetches through `releases/latest/download/`. GitHub counts that file's downloads, which is how Flatpak installs are counted ([ADR-019](DECISIONS.md#adr-019--a-flatpak-from-stets-own-repository-2026-11-12)), so a release without it both breaks the command and loses the count. Publish the site before the release, so the file points at a repository that has the version; both steps are on the owner's word:

```sh
python3 tools/flatpak.py build      # on the tagged commit
python3 tools/flatpak.py site       # needs the signing key in .local/flatpak/gpg
python3 tools/flatpak.py publish
gh release create v<version> --repo PixDevsApps/Stet --verify-tag --title "Stet <version>" \
    --notes-file <notes> .local/release/stet-x86_64.pkg.tar.zst \
    .local/flatpak/site/io.github.pixdevsapps.Stet.flatpakref
```

`site` adds the new build to the repository in `.local/flatpak/site/repo` and keeps the last three, so that folder and the key are what the next release needs; without the folder, `site` starts a new repository that holds only the new build. `PixDevsApps/stet-flatpak` (public, created on 2026-11-12) serves its `main` branch's root through GitHub Pages, set once.

Releases are never drafts or pre-releases when published: GitHub's latest release, which both the install line and Check for Updates read, skips those. Check for Updates reads only the tag, so a tag must be `v` and three plain numbers.

Under the standing authorization of 2026-10-01 local package builds are allowed; a system-wide `pacman -U` needs the user's sudo password and stays the user's step ([PLAN.md](PLAN.md#authorization-log)).

## Runtime paths

The directory name is `stet` ([ADR-010](DECISIONS.md#adr-010-accepted--stet-2026-09-30)).

| Path | Contents | From |
| --- | --- | --- |
| `$XDG_STATE_HOME/stet/` | `session.json`, `session.json.prev`, `backup/<tab id>.txt`, `backup/orphaned/`, `lock` (M2; the recent files moved into `session.json`, and M1's `recent.json` is migrated once and deleted); `save-backups/` (M3); directories 0700, files 0600. Debug and release builds share it; while one holds the lock, the other runs without a session | M1 |
| `$XDG_CONFIG_HOME/stet/` | `config.toml`, `keys.toml` | M5 |
| `$XDG_CACHE_HOME/stet/styles/` | the one generated GtkSourceView scheme (`stet-omarchy-<hash>.xml`) | M1 |
| `~/.local/state/omarchy/current/` | read and watched, never written ([ADR-004](DECISIONS.md#adr-004-amendment--as-built-in-m1-2026-10-01)) | M1 |
| `~/.config/fontconfig/` | watched; the family comes from `fc-match monospace` | M1 |
| `$XDG_STATE_HOME/omarchy/defaults/editor`, `$XDG_CONFIG_HOME/mimeapps.list` | written only by Settings › Set as Default Editor… after the user confirms (the latter through `xdg-mime default`) | M5 |
| `~/.var/app/io.github.pixdevsapps.Stet/` | the Flatpak's own `config/stet/`, `.local/state/stet/` and `cache/stet/`: Flatpak points the XDG variables there ([INTEGRATIONS.md](INTEGRATIONS.md#9-flatpak-16)) | 1.6 |
