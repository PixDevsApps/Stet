# Project Operating Guide

Project: Stet, a fast, keyboard-first text and code editor for Omarchy

Synced: 2026-09-30

The repository docs are the canonical project memory ([ADR-012](DECISIONS.md)). Dated entries describe the state at that date; newer entries take precedence.

---

## Memory rule

There is one project memory: the Markdown in this repository. [PLAN.md](PLAN.md) holds the current state, the authorization log, follow-ups and dated progress entries; [DECISIONS.md](DECISIONS.md) holds decisions; [TESTING.md](TESTING.md) and `docs/spikes/` hold evidence. Nothing important lives only in a chat, a terminal or someone's head.

## Work cycle

For every piece of work:

1. Read the current milestone and follow-ups in PLAN.md and confirm the dependencies are done.
2. Keep the slice small; the app stays runnable after it.
3. Implement with tests (see [TESTING.md](TESTING.md)).
4. Run `bash tools/ci.sh`; run GTK programs only through `tools/headless.sh <display> <cmd...>`.
5. Record evidence exactly as run: date, command, environment, numbers, caveats.
6. Update every affected doc in the same change.
7. Add a dated entry to PLAN.md when the state of the milestone changes.

Bugs and follow-ups found along the way go into PLAN.md with a priority, not into `TODO` comments.

## Milestones and preview gates

Work proceeds through milestones M0–M9 ([PLAN.md](PLAN.md#milestone-plan)), each with a Goal, Key outcomes, Exit criteria and a size in focused engineering days. A milestone ends only after its **preview gate**:

1. **Screenshots** of the current build in `tools/progress/`: captured headlessly by the app or by the user in the live session (the capture method is set up in M1). M0's gate shows the spike window and the spike reports.
2. **A dated PLAN.md entry**: what changed, which exit criteria are met with links to evidence, what is not verified, and known limits.
3. **The user's approval**, recorded with its date in that entry. Requested changes are recorded too, and the milestone stays open until they are handled or moved to a later milestone by the user.

M1 has an extra gate **before any UI work**: the [DESIGN.md](DESIGN.md) mock of the tab strip, status bar, find bar, palette and banners must be approved by the user.

Version tags (0.2 at M4, 0.3 at M5, 1.0 at M9) need git, so they follow the authorization rules below.

## Authorization gates

These actions need an explicit user authorization **before** they happen:

- `git init`, commits, pushes, tags, and creating a remote or repository (for example a private `PixDevsApps/Stet`);
- package builds (`makepkg`) and installs (`pacman -S`/`-U`, `cargo install`, anything under `/usr`);
- live-session checks by an agent (opening windows on the user's Wayland display, spike S4, the manual acceptance list).

Record each authorization in the PLAN.md authorization log with the date, the exact action and its limits. An authorization covers only what it names: permission to commit is not permission to push, and permission to build a package is not permission to install it. Downloading crates through `cargo build` needs no authorization.

## Architecture decisions

- Write an ADR for any decision with long-term cost: a dependency, a data format, a file location, a user-visible behaviour that is hard to change, a scope tier.
- Status moves `Proposed` → `Accepted` when the user decides or a spike provides the evidence; say which.
- Spike results update the ADRs they test. For example, S1–S6 either confirm ADR-002 or lead to an amendment (a large-file viewer after an S1 miss; "column mode in 1.0, or GPUI" after an S5 miss).
- Change an accepted ADR with a dated `###` amendment under it that states what it supersedes. Never rewrite an earlier entry.
- ADR-010 (name and app-id) must be Accepted before M0 closes. It was, on 2026-09-30: the name is Stet.

## Follow-up status and priority

Follow-ups in PLAN.md use these states:

- **Open:** recorded, not started.
- **In progress:** being worked.
- **Blocked:** cannot proceed; the reason is written next to it.
- **Review:** done, waiting for validation or the user's preview.
- **Done:** acceptance verified and docs updated.

Priorities:

- **P0:** required for the milestone's exit or blocks other work.
- **P1:** important for quality or release readiness; 1.0 ships with none open.
- **P2:** useful; can slip without breaking the product.
- **P3:** future or optional.

## Scope requests

New ideas go to the 1.x or Later lists in PLAN.md "Scope control". Moving an item into 1.0 is a user decision, recorded as a dated PLAN.md entry and an amendment to [ADR-013](DECISIONS.md).

## Agent safety

- Work only inside the repository; reading elsewhere is fine.
- Never open windows on the user's live Wayland display; use `tools/headless.sh` with the assigned display number, make programs self-terminate, and leave no `gtk4-broadwayd` running. Never bypass its private D-Bus configuration ([BUILD.md](BUILD.md#headless-gtk-runs) explains why).
- Never edit `~/.config`, `~/.local/state/omarchy` or system files.
- Never invent measurements or acceptance evidence.
