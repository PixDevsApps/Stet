# Contributing to Stet

Stet is a native, keyboard-first text and code editor for Omarchy, written in Rust with GTK 4, libadwaita and GtkSourceView 5. Thank you for helping. This page is for anyone opening an issue or a pull request; [AGENTS.md](AGENTS.md) adds the rules for AI coding agents working in the project owner's checkout.

The repository is [PixDevsApps/Stet](https://github.com/PixDevsApps/Stet). `main` is the only long-lived branch and releases are annotated tags (`v1.6.0`). Propose a change as a branch and a pull request against `main`.

## Before you start

- **Scope.** Stet keeps a lean feature set ([PRD](docs/PRD.md), [ADR-013](docs/DECISIONS.md#adr-013--lean-10-scope-2026-09-30)). New features go on the 1.x or Later lists in [PLAN.md](docs/PLAN.md#scope-control) first; please open an issue to discuss one before writing it.
- **Architecture.** Read [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): three crates in one direction (`domain` pure logic ← `infrastructure` std I/O ← `app` GTK), modules by bounded context, and a table of where new code goes.
- **Decisions.** [docs/DECISIONS.md](docs/DECISIONS.md) records why things are the way they are. If your change contradicts an ADR, say so in the pull request.

## Set up

On Arch or Omarchy:

```sh
sudo pacman -S --needed rust gtk4 libadwaita gtksourceview5 pcre2 pkgconf python
cargo run
```

Rust 1.92 or newer (edition 2024), GTK 4.18, libadwaita 1.9 and GtkSourceView 5.18 or newer. [docs/BUILD.md](docs/BUILD.md) has the details, the release packaging and the Flatpak.

## Checks

```sh
bash tools/ci.sh            # rustfmt, Clippy (-D warnings), tests, rustdoc, build, link check
STET_CI_DISPLAY=9 bash tools/ci.sh --ui   # plus the headless UI self-tests
```

The UI self-tests run Stet under GTK's Broadway backend through `tools/headless.sh`, with a private D-Bus session, so nothing opens on your desktop. Pick a free display number. [docs/TESTING.md](docs/TESTING.md) explains the self-test scripts.

## Making a change

1. Keep each change small and the application runnable after it.
2. Follow the layer rules: no I/O and no GTK in `domain`, no GTK in `infrastructure`, nothing that blocks the GTK thread in `app`, `unsafe` only in `app/src/sys/`. The workspace lints (`Cargo.toml`) enforce much of this.
3. Every user-visible command is an `ActionId` in `domain/src/actions/registry.rs`; menus, the command palette and shortcuts come from it.
4. Add tests with the change: unit tests, `proptest` or `insta` in `domain`; `tempfile`-based tests in `infrastructure`; a `.stet-test` script in `tests/selftest/` for UI behaviour.
5. Update the docs the change affects in the same pull request ([docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), [USER_GUIDE.md](docs/USER_GUIDE.md), [DESIGN.md](docs/DESIGN.md), …), and add a line to [RELEASE_NOTES.md](docs/RELEASE_NOTES.md) under "Unreleased" for anything a user would notice.
6. In the pull request, describe the user-visible change, what you ran, and what you could not check (for example Wayland input methods, fractional scaling or Hyprland behaviour, which headless runs don't cover).

Report measurements only from runs you actually did, with the command and machine. A check that wasn't run is "not verified", never "passed".

## Code style

Idiomatic Rust 2024, formatted by rustfmt, clean under the workspace lints. Comments say why, not when: no milestone or version tags in code. Never log the contents of documents.

## Architecture decisions

A decision with long-term cost (a dependency, a file format or location, hard-to-change behaviour) gets an ADR in [docs/DECISIONS.md](docs/DECISIONS.md): the next free number, `## ADR-NNN — Title (YYYY-MM-DD)`, with **Status**, **Decision**, **Reason**, **Alternatives** and **Consequences**. An accepted ADR is changed by a dated `###` amendment under it, never rewritten.

## Issues

For a bug, include the Stet version (`stet --version`), how you installed it (Arch package or Flatpak), your distribution and the GTK, libadwaita and GtkSourceView versions, the steps to reproduce, and a short log (`RUST_LOG=debug stet`). Never attach files that may contain private text: Stet's session and backups keep unsaved documents.

## License

Contributions are under the repository's [MIT license](LICENSE). Keep third-party notices when dependencies change (`python3 tools/license-report.py`).
