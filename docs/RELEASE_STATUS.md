# Release status: Stet 1.3.4

Project: Stet, a fast, keyboard-first text and code editor for Omarchy

Synced: 2026-10-02

The M9 exit criteria ([PLAN.md](PLAN.md#m9--polish-qa-and-10-6-8-days)) are: every checklist item passes or is recorded here, no P1 bugs, and the user's sign-off. This page is the record. Evidence, with commands, dates and caveats, is in [TESTING.md](TESTING.md); decisions in [DECISIONS.md](DECISIONS.md).

**State on 2026-10-02:** every milestone, M0–M9, is built and merged on `main`; no P1 bug is open. The source and the release tags are in the GitHub repository [PixDevsApps/Stet](https://github.com/PixDevsApps/Stet), public since 2026-10-02. **The user signed off on 2026-10-02:** "(if there is something I have not signed off, I accept and sign off now), everyting in Stet is working beautifully". The live checks only the user could run (below) were not run; the sign-off accepts them.

**1.1.0 (2026-10-01)** adds what the user asked for after installing 1.0.0: a double-click on a tab renames its file (Save As… on an untitled tab), and the tab menu has Save and Save As… ([release notes](RELEASE_NOTES.md#110--2026-10-01)). It was checked with CI on its code (701 tests; 48 self-test scripts, 4,531 steps), the live checks `tab_double_click` and `tab_menu`, and the container's fresh install of its package ([TESTING.md](TESTING.md#2026-10-01--110-a-double-click-on-a-tab-save-in-the-tab-menu)). The rest of this page is the 1.0 record and holds for 1.1.0; the other live checks were not run again.

**1.2.0 (2026-10-01)** names untitled tabs without saving, at the user's request: Rename… (formerly Rename File…), also with a double-click, gives an untitled tab a name that the session keeps ([release notes](RELEASE_NOTES.md#120--2026-10-01)). It was checked with CI on its code (701 tests; 49 self-test scripts, 4,622 steps), the live checks `tab_double_click` and `tab_menu`, and the container's fresh install of its package ([TESTING.md](TESTING.md#2026-10-01--120-names-for-untitled-tabs-without-saving)); the other live checks were not run again.

**1.3.0 (2026-10-01)** puts the hamburger menu at the left end of the tab strip, at the user's request ([release notes](RELEASE_NOTES.md#130--2026-10-01)). It was checked with CI on its code (701 tests; 49 self-test scripts, 4,635 steps), the live checks `main_menu` (new), `tab_double_click`, `tab_menu`, `split_view` and `a11y_audit`, and the container's fresh install of its package ([TESTING.md](TESTING.md#2026-10-01--130-the-menu-on-the-left)); the other live checks were not run again.

**1.3.1 (2026-10-01)** opens the menu into the window; in 1.3.0 half of it hung outside a window that didn't start at the screen's left edge ([release notes](RELEASE_NOTES.md#131--2026-10-01)). Checked with CI (701 tests; 49 scripts, 4,638 steps), the live checks `main_menu` and `tab_menu`, and the container's fresh install ([TESTING.md](TESTING.md#2026-10-01--131-the-menu-opens-into-the-window)).

**1.3.2 (2026-10-02)** makes each page of the menu only as wide as its own items and keeps the menu inside the window (a long submenu scrolls in a short window), fills in About, and makes the icon the note alone, at the user's request ([release notes](RELEASE_NOTES.md#132--2026-10-02)). Checked with CI (702 tests; 50 scripts, 4,647 steps), the live checks `main_menu`, `tab_double_click`, `tab_menu`, `split_view` and `a11y_audit`, screenshots of the menu and About, and the container's fresh install of its package ([TESTING.md](TESTING.md#2026-10-02--132-a-narrower-menu-about-and-the-icon)).

**1.3.4 (2026-10-02)** is the first public release, on GitHub as [v1.3.4](https://github.com/PixDevsApps/Stet/releases/tag/v1.3.4) with its package: the repository is public ([release notes](RELEASE_NOTES.md#134--2026-10-02)). Stet works as in 1.3.3. Checked with CI (704 tests; 50 scripts, 4,647 steps) and the container's fresh install of its package ([TESTING.md](TESTING.md#2026-10-02--134-the-repository-goes-public)).

**1.3.3 (2026-10-02)** rewords `stet --help`, the package description, the desktop entry's comment, the header of the `keys.toml` Stet creates and the README's introduction, at the user's request ([release notes](RELEASE_NOTES.md#133--2026-10-02)). Checked with CI (704 tests; 50 scripts, 4,647 steps), `desktop-file-validate` and the container's fresh install of its package ([TESTING.md](TESTING.md#2026-10-02--133-new-wording-for-stets-descriptions)).

## Milestones

| Milestone | Status | Evidence |
| --- | --- | --- |
| M0 Foundation and spikes | Closed with GO by the user (2026-10-01) | [PLAN.md](PLAN.md#m0-closed-and-the-whole-plan-authorized--2026-10-01), [spikes](spikes/) |
| M1 Themed shell, installable | Done; live checks in the live acceptance script | [PLAN.md](PLAN.md#m1-built-and-checked-headless-live-checks-pending--2026-10-01) |
| M2 Never lose work | Done; live: SUPER+W, SIGTERM, `kill -9`, `systemctl --user stop`, `GIT_EDITOR` | [PLAN.md](PLAN.md#m2-built-and-checked-headless-live-checks-pending--2026-10-01) |
| M3 Files done right | Done | [PLAN.md](PLAN.md#m3-built-and-checked-headless-live-checks-pending--2026-10-01) |
| M4 Search (0.2.0) | Done | [PLAN.md](PLAN.md#m4-search-built-and-checked-headless-live-checks-pending--2026-10-01) |
| M5 Text toolbox, the MVP (0.3.0) | Done | [PLAN.md](PLAN.md#m5-built-and-checked-headless-live-checks-pending--2026-10-01) |
| M6 Column mode | Done | [PLAN.md](PLAN.md#m6-column-mode-built-and-checked-headless-live-checks-pending--2026-10-01) |
| M7 Marking, split view, compare | Done | [PLAN.md](PLAN.md#m7-built-and-checked-headless-live-checks-pending--2026-10-01) |
| M8 Navigation, Replace in Files | Done | [PLAN.md](PLAN.md#m8-built-and-checked-headless-live-checks-pending--2026-10-01) |
| M9 Polish, QA and 1.0 | Done; signed off by the user on 2026-10-02 | [PLAN.md](PLAN.md) (the M9 entry) |

Preview gates: M0 approved by the user; M1–M8 judged by the agent from their screenshots in `tools/progress/public/` under the standing authorization of 2026-10-01.

## Automated checks

- **Headless CI** (`bash tools/ci.sh --ui`, GTK Broadway): fmt, Clippy `-D warnings`, 701 unit, property and integration tests, the build, a smoke run, and 47 self-test scripts with 4,442 steps, all passing on the 1.0.0 commit ([TESTING.md](TESTING.md#2026-10-01--m9-100-final-ci-live-acceptance-fresh-install-and-upgrade)).
- **Live acceptance** (`tools/live/acceptance.py`, the user's Hyprland session, real keymap, real pointer, AT-SPI): 36 default checks plus the opt-in D-Bus service check, all passing on the 1.0.0 build (one rerun after the focus moved to another window mid-check); the list and results are in [TESTING.md](TESTING.md). It includes an accessibility audit: every showing interactive widget has an accessible name.
- **Packaging:** the source tarball is reproducible (the same SHA-256 twice); `makepkg --cleanbuild` passes; a fresh install in an `archlinux:base` container (`tools/container-test.sh`) passes `pacman -Qkk` with 0 altered files, starts (first frame 214.4 ms headless), and keeps a user's state after `pacman -R`; the 1.0.0 package is `stet-1.0.0-1-x86_64.pkg.tar.zst`. The 1.1.0 package, `stet-1.1.0-1-x86_64.pkg.tar.zst`, passed the same checks (413 files, 0 altered; first frame 216.6 ms headless), and so did the 1.2.0 package, `stet-1.2.0-1-x86_64.pkg.tar.zst` (413 files, 0 altered; first frame 219.3 ms), the 1.3.0 package, `stet-1.3.0-1-x86_64.pkg.tar.zst` (413 files, 0 altered; first frame 223.1 ms), the 1.3.1 package, `stet-1.3.1-1-x86_64.pkg.tar.zst` (413 files, 0 altered; first frame 219.1 ms), the 1.3.2 package, `stet-1.3.2-1-x86_64.pkg.tar.zst` (413 files, 0 altered; first frame 213.2 ms), the 1.3.3 package, `stet-1.3.3-1-x86_64.pkg.tar.zst` (413 files, 0 altered; first frame 215.0 ms), and the 1.3.4 package, `stet-1.3.4-1-x86_64.pkg.tar.zst` (413 files, 0 altered; first frame 216.6 ms, as built again on 2026-10-02 from the rewritten `v1.3.4`).
- **Upgrade:** a session written by the 0.3.0 build restores in 1.0 (43 tabs: every text, selection, language, encoding and line ending equal).

## Performance targets

| Measure | Target | Result |
| --- | --- | --- |
| Cold start (`main` to first frame, release, live) | ≤ 400 ms | median 260.1 ms on the 1.0.0 build (5 launches; 284 ms before M5, 350 ms before the menu was built lazily) |
| 50-tab session restore | ≤ 500 ms | 157.5–166.9 ms (M2, release, headless); 43 tabs from a 0.3.0 session: first frame 204–210 ms |
| Keystroke on a 10 MB file | < 16 ms | 3.4 ms median, 11.7 ms worst, insert to painted frame (M3, release, headless); live: every key visible within 3 ms of its injection |
| 100 MB open, first screen | ≤ 1.5 s | 164.7 ms from the open (M3, headless); live, the 94 MiB fixture's first screen 545–706 ms after the process started |
| Regex Replace All over 1M lines | ≤ 5 s, one undo step | 2.28 s, one undo step (known gap: the GTK thread is blocked 1.9 s while the bulk edit goes in) |
| Find in Files over `~/Projects` | first results < 300 ms, instant cancel | first shown 33.8–55.6 ms; Stop within 38.4 ms (Replace in Files) |
| Format a 20 MB JSON file | UI never blocks | 1.62 s on a worker, longest main-loop block 37.0 ms (M5) |
| Column insert at column 0 across 10k lines | one undo step, < 200 ms | 12.0–15.2 ms; undo 17.7–25.8 ms, redo 18.9–27.7 ms (M6, release) |
| Compare two 10k-line files | < 1 s | 7.6–67.4 ms end to end (M7, release) |
| Mark a regex with Bookmark line, then Remove Non-Bookmarked Lines | one undo step | one step on 105,613 lines (M7) and, live, on a 200,000-line log |
| Replace in Files keeps encoding, BOM and EOL | all fixtures | 23 text fixtures byte for byte as an oracle without a codec; 9 binary and lossy ones untouched and listed (M8) |

## Manual Omarchy acceptance

The table with every row's status is in [TESTING.md](TESTING.md#manual-omarchy-acceptance). Rows the live acceptance script covers fully or in part are marked there. Not run, because they needed the user's own hands, password or hardware state; the user's sign-off of 2026-10-02 accepts them:

| Row | What | Why it is the user's |
| --- | --- | --- |
| 3, 22 | A real logout and a reboot with unsaved drafts open | ending the session |
| 4 | "Set as Default Editor…" for the real session, then SUPER+SHIFT+N and a Nautilus double-click | changes the user's real defaults |
| 5 | Drag and double-click from Nautilus itself | the script drags from a GTK test window |
| 8 | Fractional scaling at 1.25 and 1.6 | changes the monitor configuration |
| 9, 9a | fcitx5 CJK input and compose keys | needs an input-method engine; injected keys bypass fcitx5 |
| 20 | Files on a network mount | needs a server |
| 23 | `SUDO_EDITOR="stet --wait" sudoedit /etc/hosts` | needs the sudo password |
| 24 | Alt+drag with the real mouse, fcitx5 preedit in column mode | an injected key cannot hold Alt during a pointer drag |
| P4 (package) | `pacman -U` over an installed 0.3 package | needs the sudo password; the session part is verified |

## Known limitations (no P1)

- **P2:** GtkTextView validates every line's height after a big load: about 44 s of a busy main thread for a 1M-line file. Typing and painting stay responsive (blocks under 60 ms), but idle work waits ([ADR-003 amendment](DECISIONS.md#adr-003-amendment--the-first-screen-of-a-load-after-m2-2026-10-01)).
- **P3:** undoing or redoing a very large operation is one GTK step applied at once: 0.6 s for a 20 MB format, 1.5 s for 1M replacements, 6.7 s for a removal that dropped most of a large-file-mode document.
- **P3:** applying a 1M-replacement Replace All blocks the GTK thread for about 1.9 s.
- **P3:** the first frame after Compare of two large files just opened took 0.65–1.17 s (main loop never blocked over 52 ms; cause not found).
- **P3:** in very large changed blocks the diff pairs changed lines only near the block's diagonal.
- **P3:** RSS after opening the 100 MB fixture was 3.07× the file size against S1's 3× bar.
- **Accepted:** GTK 4.22 popover menu items have no accessible name; Stet names them itself ([upstream note](upstream/gtk-popover-menu-item-names.md)). AT-SPI `AddSelection` crashes GTK 4.22 text views; Stet's view is NULL-safe ([upstream note](upstream/gtk-accessible-text-add-selection.md)).
- **Not run:** a real screen reader (Orca is not installed); the AT-SPI audit stands in for it.

## For the user

1. Install system-wide (the README's Install section has the steps for anyone): `sudo pacman -U .local/release/stet-1.3.4-1-x86_64.pkg.tar.zst` from the repository folder, or the same package from the [v1.3.4 release](https://github.com/PixDevsApps/Stet/releases/tag/v1.3.4); it upgrades an installed 1.x. Then quit Stet (Ctrl+Alt+Q) and start it again: a Stet that is running keeps its old version until then. That package was built from the `v1.3.4` tree (`c1c56d4`) by `tools/container-test.sh` and passed its fresh-install test; SHA-256 `4698db65aa0174a9e57a281cc6dd56c6da681eb782a6be8aaebc4773e2fdcd81`. The 1.3.3 package (`v1.3.3`, `6d2d098`, SHA-256 `f40bcfead5c7eecaf2a42f9222db014688243d312ae11c590dac465c57e6773b`), the 1.3.2 package (`v1.3.2`, `55e9817`, SHA-256 `b263dd6f1d838ba34d07cac2185550f0a87d9c0a61364679796ebc797c1d593a`), the 1.3.1 package (`v1.3.1`, `7c8db16`, SHA-256 `12b213d8ef11c323a9c346bf786b7355a22466ed323b162c0b4f559eb01dc344`), the 1.3.0 package (`v1.3.0`, `54c6a07`, SHA-256 `a4ec4a312d29f1daa5083597444ca481b5380ad3749324376bf34b03a8f97b70`, also on the GitHub release v1.3.0), the 1.2.0 package (`v1.2.0`, `138d18c`, SHA-256 `9c161b723215bf866427a113e5aef3bb91c8174f8c20cbbc3b43be1e9ed14075`), the 1.1.0 package (`v1.1.0`, `ada2abe`, SHA-256 `9bf26af44d75e0a6aa10e963ffce9730797e1cb17e80246efb78f5a22a7a818c`) and the 1.0.0 package (`v1.0.0`, `558432a`, SHA-256 `4a3fa851d207a452165c9ca1c53d2c98116f706fd7572a5139055c250e1f9892`) are next to it. To build it yourself: [INSTALL.md](INSTALL.md).
2. Optional: report the two GTK issues upstream (texts in `docs/upstream/`), and install Orca for a screen-reader pass.
