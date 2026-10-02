# Contributing to Stet

Stet is a native, keyboard-first text editor for Omarchy. Keep changes within the
[product requirements](docs/PRD.md) and the current milestone in the [plan](docs/PLAN.md).
Ideas outside the lean 1.0 scope go on the 1.x or Later lists, not into the code.

The repository is [PixDevsApps/Stet](https://github.com/PixDevsApps/Stet) on GitHub (public since
2026-10-02); `main` is the only branch, and releases are annotated tags (`v1.3.4`). Propose changes as a
branch and a pull request against `main`.

For the original development environment, work only in
`/home/fredrick/Projects/linux-apps/notes` (the folder keeps the M0 working name).
Contributors elsewhere use their own checkout; that path is a session constraint, not a
build requirement.

## Workflow

1. Read [BUILD.md](docs/BUILD.md), the [decisions](docs/DECISIONS.md) and the current
   milestone in [PLAN.md](docs/PLAN.md).
2. Keep each change small and the application runnable after it.
3. Respect the crate boundaries: `domain` is pure logic, `infrastructure` does std I/O,
   and only `app` links GTK. Nothing blocks the GTK thread. Every user-visible command
   is an `ActionId`.
4. Add tests with the change: proptest or insta for `domain`, tempfile-based tests for
   `infrastructure`, and a `.stet-test` script in `tests/selftest/` for UI behaviour
   (`stet --self-test`, run headless; see [TESTING.md](docs/TESTING.md)).
5. Run the checks below, then describe the user-visible change, what you ran and any
   remaining limitation in the commit or PR.

Git commits, pushes, package builds and installs by agents need a dated user
authorization recorded in PLAN.md first; see [AGENTS.md](AGENTS.md).

## Checks

```sh
bash tools/ci.sh
```

The script runs `cargo fmt --all --check`, Clippy with `-D warnings`, the test suite and
a build; `--ui` adds a headless smoke run of the app. Run GTK programs headless with
`tools/headless.sh <display> <cmd...>` (Broadway backend); see [BUILD.md](docs/BUILD.md)
and [TESTING.md](docs/TESTING.md). A passing headless run does not prove Wayland,
input-method, fractional-scaling or Hyprland behaviour; those are on the manual Omarchy
acceptance list.

Report measurements only from runs you actually did, with the date, command, machine and
caveats. Never mark an unrun check as passed.

## Architecture decisions

Decisions with long-term cost get an ADR in [DECISIONS.md](docs/DECISIONS.md):

- Use the next free number and the heading `## ADR-NNN — Title (YYYY-MM-DD)`.
- Write **Status**, **Decision**, **Reason** and **Consequence**; add **Alternatives** and
  **References** (authoritative links with versions) when they help.
- Status is `Proposed`, `Accepted` (say by whom: user decision, spike evidence),
  `Superseded by ADR-NNN` or `Rejected`.
- Change an accepted decision with a dated `###` amendment under it that says what it
  supersedes. Do not rewrite history.

## Documentation sync rule

The repository docs are the canonical project memory ([ADR-012](docs/DECISIONS.md)).

- A change that alters behaviour, scope, architecture, commands or acceptance updates
  the affected docs in the same change.
- Progress, preview gates and authorizations are dated entries in PLAN.md, newest first.
  Older entries stay as history; newer entries take precedence.
- Keep the "Synced:" date at the top of each doc current when you edit it.

## Issues

For bugs, include the version or commit, Omarchy and GTK/libadwaita/GtkSourceView
versions, steps to reproduce and a short log. Never attach file contents that may contain
secrets; backups and sessions can hold unsaved private text.

## Licenses

Original contributions use the repository's [MIT license](LICENSE). Keep third-party
notices when dependencies change.
