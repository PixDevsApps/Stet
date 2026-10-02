# Stet agent operating guide

Work only in `/home/fredrick/Projects/linux-apps/notes`. Reading other paths (for example `/home/fredrick/Projects/linux-apps/hertz` or `/usr/share/omarchy`) is fine; do not create a parallel repository.

The product is **Stet** ([ADR-010](docs/DECISIONS.md), accepted 2026-09-30). "Notes" was the working name during research and M0: the project folder keeps it, and the spike reports and older dated entries still use it.

## Current scope

**Stet 1.0.0** (2026-10-01): every milestone, M0–M9, is built, merged and checked, headless and with the live acceptance script. **1.1.0** (2026-10-01) adds, at the user's request, a double-click on a tab to rename its file and Save and Save As… in the tab menu; **1.2.0** (2026-10-01) names untitled tabs without saving (Rename…); **1.3.0** (2026-10-01) puts the hamburger menu at the left end of the tab strip, **1.3.1** opens it into the window, **1.3.2** (2026-10-02) makes each menu page only as wide as its items, fills in About and makes the icon the note alone, **1.3.3** rewords `stet --help`, the package description, the desktop entry and the README's introduction, and **1.3.4** is the first public release. The history starts at 1.3.4: `v1.3.4` is the only tag. The repository's remote `origin` is the GitHub repository `PixDevsApps/Stet` (created 2026-10-01, public since 2026-10-02); each push still needs the user's authorization. The user signed off on 2026-10-02, accepting the manual checks only the user could run without their being run ([docs/RELEASE_STATUS.md](docs/RELEASE_STATUS.md)). Work after 1.0 follows the 1.x list in PLAN.md's Scope control and needs the user's decision. [docs/PLAN.md](docs/PLAN.md) holds the current state, the milestone plan and the authorization log.

## Project memory

The repository docs are the canonical project memory ([ADR-012](docs/DECISIONS.md)). There is no Notion project and no external tracker.

Before substantive work, read [PLAN.md](docs/PLAN.md), [DECISIONS.md](docs/DECISIONS.md), [PRD.md](docs/PRD.md) and [OPERATING.md](docs/OPERATING.md).

- Record progress as dated entries in PLAN.md, newest first.
- Record decisions with long-term cost as ADRs; amend with dated sub-entries instead of rewriting history.
- Update the affected docs in the same change as the code.
- Record follow-ups in PLAN.md instead of hiding TODOs in code.

## Authorization gates

- Every `git init`, commit, push, remote or repository creation, package build (`makepkg`) and install (`pacman -S`/`-U`, `cargo install`, anything under `/usr`) needs an explicit authorization from the user, recorded with its date in the PLAN.md authorization log **before** acting. An authorization covers only what it names.
- Downloading crates through `cargo build` needs no authorization.
- Never edit `~/.config`, `~/.local/state/omarchy` or system files. The app itself only does so through opt-in commands the user confirms at runtime.

## Evidence

Never invent measurements, acceptance evidence, commit references or links. Record only what was actually run: date, command, machine and caveats. Do not silently ignore test failures. "Not verified" is an acceptable result; a guessed number is not.

## Architecture rules

- Dependency direction: `stet-domain` ← `stet-infrastructure` ← `stet` (the app).
  - `domain/`: pure logic. No I/O, no GTK, no threads.
  - `infrastructure/`: std I/O only, testable headless.
  - `app/`: the only production crate that links GTK, libadwaita and GtkSourceView.
  - `spikes/` (`stet-spikes`): M0 measurement code. Production crates never depend on it.
- Nothing blocks the GTK thread. File I/O, encoding, search, backups and formatting run on worker threads with owned snapshots; results return through `async-channel` and stale results are dropped.
- Every user-visible command is an `ActionId`. The hamburger menu, F1 palette and accelerators are generated from the action registry.
- The GtkSourceView buffer is the source of truth; an operation of more than about 2,000 edits is applied as one bulk edit ([ADR-003](docs/DECISIONS.md)).
- Every replace runs through our own `pcre2` engine. Never call `SearchContext::replace_all` or `SearchContext::replace` ([ADR-005](docs/DECISIONS.md), [upstream bug](docs/upstream/sourceview5-replace-all.md)).
- Never log document contents.
- Idiomatic Rust 2024, MSRV 1.92, `clippy -D warnings` clean, no unnecessary comments; match the surrounding style.

## Product constraints

- Follow the live Omarchy theme and font ([ADR-004](docs/DECISIONS.md)). The only built-in palettes are the fallbacks for running outside Omarchy.
- The classic editor keymap with Omarchy remaps ([ADR-009](docs/DECISIONS.md)).
- Lean 1.0 ([ADR-013](docs/DECISIONS.md)). Items on the 1.x, Later and Never lists in PLAN.md "Scope control" are not built before 1.0 without a new user decision.
- App-id: `io.github.pixdevsapps.Stet.Devel` for debug builds, `io.github.pixdevsapps.Stet` for release builds. Never `org.omarchy.*` or `TUI.*`.
- The app must also run outside Omarchy.

## Running checks

- `bash tools/ci.sh`: formatting, Clippy with `-D warnings`, tests and build. `bash tools/ci.sh --ui` adds a headless smoke run of the app and the `stet --self-test` scripts in `tests/selftest/`; set `STET_CI_DISPLAY` to your assigned display number.
- UI behaviour gets a `.stet-test` script ([TESTING.md](docs/TESTING.md#ui-self-tests-stet---self-test-from-m1)).
- **Never open windows on the user's live Wayland display.** Run every GTK program headless through `tools/headless.sh <display> <cmd...>` (GTK Broadway backend plus a private D-Bus session) with the display number you were assigned. Make the program terminate itself (`STET_EXIT_AFTER_MS` for the app; the wrapper also enforces `STET_HEADLESS_TIMEOUT`) and leave no `gtk4-broadwayd` process running. Never set `STET_SPIKE_ALLOW_LIVE=1` without explicit authorization.
- **Never bypass the wrapper's private D-Bus configuration** (`tools/headless-dbus.conf`). It gives a bus that cannot start any service. On 2026-09-30, during M0, a plain private `dbus-run-session` auto-started xdg-desktop-portal-hyprland, which connected to the live Hyprland session. Details: [BUILD.md](docs/BUILD.md#headless-gtk-runs).
- Live-session checks (spike S4, the manual Omarchy acceptance list) are done by the user, or by an agent only with explicit authorization.

See [BUILD.md](docs/BUILD.md) and [TESTING.md](docs/TESTING.md) for commands.
