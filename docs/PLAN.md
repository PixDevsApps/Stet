# Implementation Plan

Project: Stet, a fast, keyboard-first text and code editor for Omarchy

Source: the plan the user approved on 2026-09-30 (`~/.claude/plans/i-want-to-create-vivid-donut.md`, outside the repository). This file is its canonical repository form.

Synced: 2026-10-02

The repository docs are the canonical project memory ([ADR-012](DECISIONS.md)). Dated entries describe the state at that date; newer entries take precedence.

---

## The history starts at 1.3.4 — 2026-10-02

Asked how to go on, the user chose "Start history fresh" (authorization log, same date).

- **History:** `main` starts again with the commit `Stet 1.3.4`, which holds the files as they were that day, and the annotated tag `v1.3.4` points to it. The earlier commits and the tags `v0.2.0`–`v1.3.3` are no longer in the repository; a bundle of the earlier history is kept outside it, in the ignored `.local/history/`. Commit IDs in the entries below and in the other docs' dated records name commits of the earlier history.
- **Checks** ([TESTING.md](TESTING.md#2026-10-02--the-history-starts-at-134)): CI with the headless self-tests passed on the new files.
- **On GitHub:** `main` and `v1.3.4` force-pushed; the tags `v0.2.0`–`v1.3.3` and the release v1.3.0 deleted; the release v1.3.4 follows its tag.

**For the user:** a clone made before this needs a fresh clone.

## No co-author line in the history — 2026-10-02

The user asked, with a screenshot of the repository's Contributors box: "Is it possible to remove Claude as contributor?" Told that every commit message ended with a `Co-Authored-By: Claude …` line, which GitHub shows as a contributor, the user chose "Remove it everywhere" (authorization log, same date).

- **Rewrite:** `git filter-branch` over all 103 commits: each message without that line, and in the docs the commit IDs they name mapped to the new ones. Authors, committers and dates are as before, and so is every file outside the docs, in `main` and in every tag. The tags moved with their commits: `v0.2.0` `1a60210`, `v0.3.0` `6dd83f0`, `v1.0.0` `558432a`, `v1.1.0` `ada2abe`, `v1.2.0` `138d18c`, `v1.3.0` `54c6a07`, `v1.3.1` `7c8db16`, `v1.3.2` `55e9817`, `v1.3.3` `6d2d098`, `v1.3.4` `c1c56d4`.
- **From now on** commits carry no co-author line.
- **Evidence from before:** a source tarball made with `git archive` carries its commit's ID, so one made from a rewritten commit differs from the recorded checksum; the packages are unchanged.
- **On GitHub:** `main` and the ten tags force-pushed with this record; the releases v1.3.0 and v1.3.4 follow their tags, and their notes name the new commit IDs. GitHub's Contributors box can take a while to catch up.
- **Contributors box:** after the push it still listed Claude. At the user's request ("do the default branch switch now") the default branch was switched to a temporary branch at `main`'s commit and back to `main` (13:28); GitHub recomputed its contributor statistics, which name only PixDevsApps, and at 13:30 the box showed one contributor.

**For the user:** a clone made before this needs `git fetch` and `git reset --hard origin/main`, or a fresh clone.

## The history from 1.3.1 on rewritten — 2026-10-02

Asked whether to rewrite the public history, the user chose "Rewrite the history"; asked about the v1.3.4 release's package, "Rebuild and replace it" (authorization log, same date). Files that don't belong in the repository were removed from every commit since 1.3.1, with every mention of them in the docs and commit messages.

- **Rewrite:** `git filter-branch` over the commits from 1.3.1 to the previous entry's, which all have new IDs; the tags `v1.3.1`–`v1.3.4` moved with them and now point to `7c8db16`, `55e9817`, `6d2d098` and `c1c56d4`. A commit of earlier the same day that had removed those files from the newest tree only is gone. In every commit, Stet's code and every file outside the docs are byte for byte as before; the docs changed only where they mentioned those files, and where they name a rewritten commit, which they now give by its new ID.
- **Evidence from before:** the records of 1.3.1–1.3.4 name the new IDs, but the source tarballs and packages they describe were made from the trees as they were; a tarball made from a rewritten commit differs from the recorded one, because its docs do.
- **The 1.3.4 package** was built again from the rewritten `v1.3.4` and passed the container's fresh-install test ([TESTING.md](TESTING.md#2026-10-02--the-history-from-131-on-rewritten-the-134-package-built-again)): `stet-1.3.4-1-x86_64.pkg.tar.zst`, 3,740,493 bytes, SHA-256 `4698db65…`. Its `stet` binary is byte for byte the one released before; only the README and THIRD_PARTY.md it ships differ. It replaces the release's package, and the release notes give its checksum.
- **On GitHub:** `main` and the four tags force-pushed with this record; the release v1.3.4 follows its tag.

**For the user:** an installed 1.3.4 needs nothing, as the program is the same. A clone made before this needs `git fetch` and `git reset --hard origin/main`, or a fresh clone.

## The user signs off; install steps in the README — 2026-10-02

The user wrote in chat, with a screenshot of the README's Status section on GitHub: "I had a look at the repo, I dont think there is any link/code fore users to copy into the terminal to install Stet. If there is, make it a bit more obvious, whit instructions for unexperienced users. Ther is alos a bit too much info here (screenshot), don't need anything about how the name was made or the sign offs (if there is something I have not signed off, I accept and sign off now), everyting in Stet is working beautifully".

- **Sign-off:** the user signed off on Stet as it stands, 1.3.4 and everything before it: M9's last exit criterion ([RELEASE_STATUS.md](RELEASE_STATUS.md)). The manual Omarchy checks that only the user could run were not run; the sign-off accepts them, and their rows in [TESTING.md](TESTING.md#manual-omarchy-acceptance) stay empty or partial, as that table's rule asks.
- **README:** an Install section after the screenshot, for people new to a terminal: open one, paste three lines (download the package from the v1.3.4 release with `curl`, install it with `sudo pacman -U`, delete the download), what the password and pacman prompts look like, and how to start Stet, make it the default editor, update it and remove it. The Status section is now two sentences (the latest release, its notes, no AUR package yet); the name's history, the sign-off and the list of versions are gone from it, as they are in the release notes and this plan. The feature list gained what only the Status listed (quick open, the switcher, back and forward, split view and compare, bookmarks and Mark, files up to 256 MiB) and lost "(in 1.0)".
- **INSTALL.md** downloads the package with `curl` and explains the same prompts, with the GitHub CLI as an alternative; **BUILD.md** says that the README's and INSTALL.md's install lines move to each new release.
- **Checked:** the README's `curl` line, run in a scratch folder, downloaded the release's package without signing in: 3,740,766 bytes, SHA-256 `b5c994aa…` as recorded, `stet` 1.3.4-1 with its dependencies all in the Arch repositories. `pacman -U` was not run; it needs the user's password.
- **On GitHub:** asked whether to push, the user chose "Push only the README": this commit was pushed on top of `1da9b81` (authorization log, same date).

## Stet 1.3.4: the repository goes public — 2026-10-02

The user asked in chat to push, make the release and make the repository public (authorization log, same date).

**For the public repository:** README, INSTALL, CONTRIBUTING, BUILD, AGENTS and RELEASE_STATUS no longer call it private, and point to the v1.3.4 release.

**Checks** ([TESTING.md](TESTING.md#2026-10-02--134-the-repository-goes-public)): CI passed (704 tests; 50 self-test scripts, 4,647 steps) on its second run. The first stopped in `compare-banner.stet-test`, which then passed 5 of 5 runs alone. **Version 1.3.4**, tag `v1.3.4` on `c1c56d4`; the package built from it, `stet-1.3.4-1-x86_64.pkg.tar.zst` (SHA-256 `b5c994aa…`), passed the fresh-install test in the container and is in `.local/release/`.

**On GitHub** (authorization log, same date): the repository's description is now "Fast, keyboard-first text and code editor for Omarchy — Rust, GTK 4, libadwaita and GtkSourceView 5", and the v1.3.0 release's first line describes Stet the same way. `main` was pushed as a fast-forward (`818dd7e..1dd16b7`) with the annotated tags `v1.3.1`–`v1.3.4`; `git ls-remote origin` showed each tag on its commit (`7c8db16`, `55e9817`, `6d2d098`, `c1c56d4`). The release [Stet 1.3.4](https://github.com/PixDevsApps/Stet/releases/tag/v1.3.4) was created with `gh release create --verify-tag` at 11:10 CEST, not a draft or pre-release, marked Latest, with the package attached; downloaded again with `gh release download`, its SHA-256 matched. Then the repository was made public (`gh repo edit --visibility public`). Without credentials, the repository and release pages answered 200. This record was pushed after it.

**For the user:** `sudo pacman -U .local/release/stet-1.3.4-1-x86_64.pkg.tar.zst` from the repository folder, or the package from the release, then quit Stet (Ctrl+Alt+Q) and start it again.

**Follow-up (P3):** `compare-banner.stet-test` failed once: right after Keep Mine its `assert-no-banner "changed on disk"` still saw the banner, probably raised again by a late file event from the write before it. The test, or the banner after Keep Mine, should allow for that.

## Stet 1.3.3: new wording for Stet's descriptions — 2026-10-02

After 1.3.2 the user was offered new wording for `stet --help`, the package description, the launcher's description line and the README's title line, and accepted it in chat. Built under the standing authorization (authorization log, same date). The answer was read as one to that offer, not to the question about pushing in the same message.

**What changed:**
- `stet --help`: "A fast, keyboard-first text and code editor for Omarchy", About's wording.
- The package description: "Fast, keyboard-first text and code editor for Omarchy".
- The desktop entry's comment: "Edit text and code files, keyboard-first"; INTEGRATIONS shows it.
- The first line of the `keys.toml` that Settings › Open Keyboard Shortcuts creates: "your changes to Stet's default keys". Changed in a second commit after the tag; the tag moved to it (nothing was pushed) and the package was built again.
- The README's introduction: the title line, the paragraph and list under it, and the status. The keymap line now reads "familiar keyboard shortcuts, your own in `keys.toml`".
- **Tests:** unit tests for `stet --help`, for the desktop entry and package description, and for the `keys.toml` header, each failing on the old text.

**Checks** ([TESTING.md](TESTING.md#2026-10-02--133-new-wording-for-stets-descriptions)): CI passed on the final code (704 tests; 50 self-test scripts, 4,647 steps); `desktop-file-validate` gives only the old categories hint. **Version 1.3.3**, tag `v1.3.3` on `6d2d098`; the package built from it, `stet-1.3.3-1-x86_64.pkg.tar.zst` (SHA-256 `f40bcfea…`), passed the fresh-install test in the container and is in `.local/release/`, in place of the one built before the `keys.toml` change.

**For the user:** `sudo pacman -U .local/release/stet-1.3.3-1-x86_64.pkg.tar.zst` from the repository folder (it includes 1.3.2), then quit Stet (Ctrl+Alt+Q) and start it again.

## Stet 1.3.2: a narrower menu, About and the icon — 2026-10-02

The user asked in chat, with three screenshots: "The box opening when clicking the hamburger meny is a bit too wide, and it extends outside the apps tile (screenshot 1), it should open inside the tile. I can see that the box width is the same throughout all menu items, if it is a too big operation to adapt to content, skip it." Then for more details in About (screenshot 2), and: "For the program icon, I only want that note part, not the box around it (Screenshot 3)". Built under the standing authorization (authorization log, same date).

**What changed** ([DESIGN.md](DESIGN.md#as-built-in-132--2026-10-02)):
- **Screenshot 1 was 1.3.0.** The user had installed the 1.3.1 package, but the Stet running since 08:21 was still 1.3.0 (`/proc/<pid>/exe` pointed at the deleted binary), and 1.3.1 already opens the menu into the window. Told the user to quit Stet and start it again.
- **The menu:** each page is as wide as its own items. GTK's popover menu keeps its pages in a stack as wide as the widest page; Stet turns that off (`hhomogeneous`). Live, in a window 1,100 px wide: the main page is 127 px (was 418, the width of Edit › Line Operations, the widest page), File 270, Search 334, Edit 365. The height is limited to the room below the button, so a long page scrolls in a short window instead of reaching past its bottom edge.
- **About:** Details says what Stet is and does, in three paragraphs; Website and Report an Issue point to the GitHub repository, which is private, so they open only for accounts with access; Legal has the copyright.
- **The icon:** the note alone, 1.2 times its old size, with a thin edge for light backgrounds; the PNG sizes exported again. The installed icon changes with the package, so About in the live checks still showed the 1.3.1 icon.
- **Tests:** `assert-menu-pages` (new, in `split.stet-test`) shows the main page, the widest and the tallest and checks each width and that the menu is inside the window; `assert-menu-within` checks the height too. `about.stet-test` (new) opens About and checks its text with `assert-dialog-text`, which reads About too. A unit test checks that the About text is valid markup. The live check `main_menu` also opens the Search submenu.

**Checks** ([TESTING.md](TESTING.md#2026-10-02--132-a-narrower-menu-about-and-the-icon)): each new self-test failed on the old code and passes on the new; CI passed (702 tests; 50 self-test scripts, 4,647 steps); the live checks `main_menu`, `tab_double_click`, `tab_menu`, `split_view` and `a11y_audit` passed with the 1.3.2 release build, and screenshots of the menu's pages, of About and of its Details were checked by eye. **Version 1.3.2**, tag `v1.3.2` on `55e9817`; the package built from it, `stet-1.3.2-1-x86_64.pkg.tar.zst` (SHA-256 `b263dd6f…`), passed the fresh-install test in the container and is in `.local/release/`.

**Not done:** pushing and a GitHub release (they need the user's authorization).

**For the user:** `sudo pacman -U .local/release/stet-1.3.2-1-x86_64.pkg.tar.zst` from the repository folder, then quit Stet (Ctrl+Alt+Q) and start it again.

## Stet 1.3.1: the menu opens into the window — 2026-10-01

The main menu showed half outside Stet's window, a 1.3.0 bug, whenever the window didn't start at the screen's left edge. Fixed in **1.3.1**: the menu's popover opens from the button's left edge ([DESIGN.md](DESIGN.md#as-built-in-13--2026-10-01)). The self-test step `assert-menu-within` in `split.stet-test` failed before the fix ("the menu spans -25–393 in a window 1024 wide") and passes after it.
- **1.3.1:** tag `v1.3.1` on `7c8db16`; CI passed (701 tests; 49 scripts, 4,638 steps), the live checks `main_menu` and `tab_menu` passed, and the package `stet-1.3.1-1-x86_64.pkg.tar.zst` passed the fresh-install test in the container ([TESTING.md](TESTING.md#2026-10-01--131-the-menu-opens-into-the-window)).
- **Not done:** 1.3.1 is not pushed and has no GitHub release yet.

**For the user:** `sudo pacman -U .local/release/stet-1.3.1-1-x86_64.pkg.tar.zst` from the repository folder.

## GitHub release v1.3.0 — 2026-10-01

At the user's request ("yes, attach the 1.3.0 package to a GitHub release"; authorization log, same date): the release Stet 1.3.0 on the pushed tag `v1.3.0`, created with `gh release create --verify-tag` and published at 18:22 CEST, not a draft or pre-release, marked Latest.

- **Asset:** `stet-1.3.0-1-x86_64.pkg.tar.zst`, 3,738,742 bytes: the container-tested package from `.local/release/`. Downloaded again with `gh release download`, its SHA-256 matched, `a4ec4a312d29f1daa5083597444ca481b5380ad3749324376bf34b03a8f97b70`.
- **Notes:** what is new in 1.3.0 and since 1.0.0, the `pacman -U` install, the SHA-256, the build and its container test, with links to RELEASE_STATUS, TESTING and RELEASE_NOTES at `v1.3.0`.
- The repository is private, so only accounts with access see the release. The package was built before the PKGBUILD had its `url`, so its `.PKGINFO` has none.
- This docs change was committed on `main` and pushed after the user authorized it ("yes, push it").

## GitHub repository PixDevsApps/Stet — 2026-10-01

The user asked in chat: "Please update/create all necessary docs, create a new GitHub repo, "Stet", commit and push" (authorization log, same date).

- **Repository:** [`PixDevsApps/Stet`](https://github.com/PixDevsApps/Stet), **private**, like `PixDevsApps/Hertz`; the approved plan proposed a private `PixDevsApps/<Name>`. The user can make it public in its settings. The remote `origin` uses HTTPS through `gh`, signed in as PixDevsApps.
- **Pushed:** `main` (`2d8cd75`, the docs commit) and the annotated tags `v0.2.0`, `v0.3.0`, `v1.0.0`, `v1.1.0`, `v1.2.0` and `v1.3.0`; `git ls-remote origin` showed each tag on its release commit (`1a60210`, `6dd83f0`, `558432a`, `ada2abe`, `138d18c`, `54c6a07`), and GitHub detects the MIT license. This record was pushed after it. Not done: no GitHub release with the packages, no GitHub Actions (the UI self-tests need a GTK display; `tools/ci.sh --ui` stays local), no AUR package.
- **Docs for it:** README (a 1.3 screenshot, the repository, `git clone`), INSTALL (getting the source), CONTRIBUTING (branches and pull requests), BUILD and `packaging/PKGBUILD.in` (`url`), AGENTS (the remote; each push still needs an authorization), RELEASE_STATUS, ASSETS (the screenshot's origin, rendered by the new `preview-readme.stet-test`), TESTING, and the Synced lines of USER_GUIDE and INSTALL.
- **Checked before the push:** the tracked files hold no secrets or personal data (the only e-mail address is an `example.com` test value); no blob in the history is over 5 MB (4.1 MiB packed); the commits carry the author e-mail that `PixDevsApps/Hertz` was pushed with.

## Stet 1.3.0: the menu on the left — 2026-10-01

After 1.2.0 the user asked in chat: "want the hamburger menu to be on the left side". Built under the standing authorization of 2026-10-01 (commits, tag, package build, live checks); the layout change is the user's decision.

**What exists** ([DESIGN.md](DESIGN.md#as-built-in-13--2026-10-01)):
- The hamburger menu sits at the left end of the tab strip, the start of the first strip on screen; in split view it moves to the second strip while the main view has no tab (`update_layout`). Outside Hyprland the window controls the decoration layout puts first go before it, the others stay at the end of the last strip.
- **Self-tests:** `assert-menu-at` (the strip with the menu, and the menu left of its first tab); `split.stet-test` checks the menu's place in one view, in a split, with the main view empty, and split again.
- **Live:** the new check `main_menu` clicks the menu with the real pointer, after checking that it is left of the first tab.
- **Version 1.3.0**, tag `v1.3.0` on `54c6a07`. The package built from it, `stet-1.3.0-1-x86_64.pkg.tar.zst`, passed the fresh-install test in the container and is in `.local/release/`.

**Checks** ([TESTING.md](TESTING.md#2026-10-01--130-the-menu-on-the-left)): CI passed (701 tests; 49 self-test scripts, 4,635 steps); the live checks `main_menu`, `tab_double_click`, `tab_menu`, `split_view` and `a11y_audit` passed with the 1.3.0 release build; the container test passed.

**For the user:** `sudo pacman -U .local/release/stet-1.3.0-1-x86_64.pkg.tar.zst` from the repository folder upgrades the installed version.

## Stet 1.2.0: names for untitled tabs without saving — 2026-10-01

After 1.1.0 the user asked in chat for a way to give each tab a name of its own without saving, so that several open tabs are easier to tell apart without going through the Save or Save As dialog for each. Built under the standing authorization of 2026-10-01 (commits, tag, package build, live checks); the scope is the user's decision ([ADR-013 amendment](DECISIONS.md#adr-013-amendment--names-for-untitled-tabs-12-2026-10-01)).

**What exists** ([DESIGN.md](DESIGN.md#as-built-in-12--2026-10-01)):
- **Rename…** (Rename File… until 1.1) names an untitled tab without saving; on a file it renames the file as before. A double-click on an untitled tab opens it, in place of 1.1's Save As…. The name replaces the first-line name everywhere the document is named, also in clones; Save As… proposes it; an empty name follows the first line again.
- **Kept** in the session (`custom_name`, [ADR-006 amendment](DECISIONS.md#adr-006-amendment--names-for-untitled-tabs-12-2026-10-01)), for restored tabs whose text isn't loaded yet too, and by Restore Closed Tab.
- **Self-tests:** `tab-names.stet-test` (new, with the children `session/tab-names-make` and `tab-names-restore`); `tab-double-click` names an untitled tab; `context` has the new label; the step `assert-chooser-name` reads the name a Save chooser proposes.
- **Live:** `tab_double_click` names an untitled tab with a real double-click; `tab_menu` uses Rename….
- **Version 1.2.0**, tag `v1.2.0` on `138d18c`. The package built from it, `stet-1.2.0-1-x86_64.pkg.tar.zst`, passed the fresh-install test in the container and is in `.local/release/`.

**Checks** ([TESTING.md](TESTING.md#2026-10-01--120-names-for-untitled-tabs-without-saving)): CI passed (701 tests; 49 self-test scripts, 4,622 steps); the live checks `tab_double_click` and `tab_menu` passed with the 1.2.0 release build; the container test passed.

**For the user:** `sudo pacman -U .local/release/stet-1.2.0-1-x86_64.pkg.tar.zst` from the repository folder upgrades the installed version.

## Stet 1.1.0: double-click a tab to rename, Save in the tab menu — 2026-10-01

After installing 1.0.0 the user asked in chat: "I would like when double clicking on a tab the user can rename the file", then, with a screenshot of the tab menu, "It should also have a save as and save option in the menu when right clicking a tab". Built under the standing authorization of 2026-10-01, which covers the commits, the tag, the local package build and the live checks; the scope is the user's decision ([ADR-013 amendment](DECISIONS.md#adr-013-amendment--two-additions-after-10-2026-10-01)).

**What exists:**
- **A double-click on a tab** opens Rename File… for its file; on an untitled tab, Save As…; a tab still loading and a comparison's text ignore it. Both presses must be on the same tab and not on its buttons. How it finds the tab, and what a libadwaita update could break: [DESIGN.md](DESIGN.md#as-built-in-11--2026-10-01).
- **The tab menu** has Save and Save As… after the close commands; they act on the tab that was clicked (`Window::context_page`), and Ctrl+S and Ctrl+Alt+S act on the current tab as before.
- **Self-tests:** `tab-double-click.stet-test` (new, 64 steps; the command `tab-press` runs Stet's handling of a press on a tab's title or close button or on the strip after it); `context.stet-test` checks both menu items and saves a background tab and an untitled one from its menu (148 steps). `choose-file` now also drives a Save chooser: GTK 4.22's keeps its suggested name when given a file, so the folder and the name are set apart.
- **Live:** the check `tab_double_click` double-clicks with the real pointer (the pointer tool's new `double-click`); `tab_menu` also saves from the menu.
- **Version 1.1.0**, tag `v1.1.0` on `ada2abe`. The package built from it, `stet-1.1.0-1-x86_64.pkg.tar.zst`, passed the fresh-install test in the container and is in `.local/release/`.

**Checks** ([TESTING.md](TESTING.md#2026-10-01--110-a-double-click-on-a-tab-save-in-the-tab-menu)): CI passed on the final code (701 tests; 48 self-test scripts, 4,531 steps); the live checks `tab_double_click` and `tab_menu` passed with the 1.1.0 release build, after one `tab_menu` run that waited too briefly for a terminal to start; the container test passed.

**For the user:** `sudo pacman -U .local/release/stet-1.1.0-1-x86_64.pkg.tar.zst` from the repository folder upgrades the installed 1.0.0.

## M9 done: Stet 1.0.0 — 2026-10-01

Under the standing authorization of 2026-10-01. M7 was merged (`b1fa24c`) and every milestone is now on `main`; M9's polish, QA and release work followed. Evidence: [TESTING.md](TESTING.md#2026-10-01--m9-100-final-ci-live-acceptance-fresh-install-and-upgrade); the release record: [RELEASE_STATUS.md](RELEASE_STATUS.md); the notes: [RELEASE_NOTES.md](RELEASE_NOTES.md).

**Done in M9:**
- **Performance:** the main menu is built when it opens (`b76c019`): startup median 260–272 ms live (bar 400 ms; 350 ms before). The remaining performance limits are recorded in RELEASE_STATUS (no P1).
- **Accessibility:** an AT-SPI audit (`a11y_audit` in the live script) found unnamed controls and every popover menu item unnamed (a GTK 4.22 gap); all are named now (`6d004f1`, [upstream note](upstream/gtk-popover-menu-item-names.md)). Orca is not installed, so no screen-reader pass was run.
- **Themes:** compare's kinds and the token styles stay apart in single-hue themes such as the user's `dark-music2` ([ADR-004 amendment](DECISIONS.md#adr-004-amendment--role-colours-a-theme-cant-tell-apart-m9-2026-10-01)).
- **Fixes:** two GTK CRITICALs (untitled language guessing, the document map shown again); a self-test that checked the Column Editor's focus before its field was mapped.
- **Docs:** USER_GUIDE for 1.0 (navigation, Replace in Files, sessions, column mode, M7's sections, the keymap table from the registry), RELEASE_STATUS and RELEASE_NOTES.
- **Release:** version 1.0.0; final CI (701 tests, 4,442 self-test steps); all 36 live checks and the D-Bus service check; the reproducible tarball, `makepkg`, and the fresh install in a container (P2, P3, P5); the session upgrade from `v0.3.0` (P4's session part). Tag `v1.0.0` on the commit that records this.
- **Preview gates:** M2, M5, M6, M7 and M8 judged from their screenshots; all legible and consistent in the three themes, M6's block fill subtle in tokyo-night but the theme's selection colour.
- **Cleanup:** the agents' worktrees (about 35 GB) and their merged branches removed; the test leftovers in `~/.local/state/stet` and `~/.cache/stet` removed (Stet was never installed or used there for real).

**Exit criteria:** checklist items pass or are recorded in RELEASE_STATUS — met; no P1 bugs — met; **the user's sign-off — pending.**

**For the user:** the manual rows in [RELEASE_STATUS.md](RELEASE_STATUS.md#manual-omarchy-acceptance) (logout and reboot, Set as Default Editor for real, Nautilus, fractional scaling, fcitx5 and compose, a network mount, `sudoedit`, Alt+drag, `pacman -U`), the system install, and the sign-off. Optional: the GTK upstream reports in `docs/upstream/`, an Orca pass.

## M7 built and checked headless; live checks pending — 2026-10-01

Built in the M7 worktree from `123381c` (`main` with M2, M5, M6 and M8 merged) under the standing authorization of 2026-10-01. All evidence is headless (GTK Broadway, display 61); what needs the live session is at the end.

**What exists:**
- **Bookmarks** (Search › Bookmark; [USER_GUIDE.md](USER_GUIDE.md#bookmarks-and-mark)): GtkSourceView source marks at line starts, drawn in the gutter as a disc in the theme's accent colour; a click in the gutter's mark column toggles one. Toggle (Ctrl+Alt+K, Ctrl+F2), Next and Previous (Ctrl+Alt+L and J, F2 and Shift+F2; around the ends; jumps Go Back returns from), Clear All, Inverse; Cut, Copy and Remove Bookmarked Lines, Remove Non-Bookmarked Lines and Paste to (Replace) Bookmarked Lines (`domain::marks::lines`), each one undo step (one bulk replacement above 2,000 edits), on a worker above 512 KiB. Bookmarks move with typing, and every edit Stet makes itself (Replace All in the document, in open documents and Replace in Files' open documents; the text tools, also in pieces; a reload from disk; large column edits) sets them by line afterwards and keeps a step that brings them back after GTK's Undo and Redo ([ADR-003 amendment](DECISIONS.md#adr-003-amendment--bookmarks-and-marks-across-bulk-edits-undo-and-redo-m7-2026-10-01)). The session keeps them.
- **Mark** (Search › Mark…, Ctrl+M): the find bar's Mark row with the style (Find Mark Style, 1st–5th Style), Bookmark line, Purge for each search, Mark All (also Enter in the field), Clear All Marks and Copy Marked Text; the find row's mode and options apply, through our own engine (on a worker above 512 KiB). Marks are a model in the document, moved through every edit and kept across Undo and Redo of Stet's steps, and drawn as text tags in the theme's colours around the lines on screen, recoloured on theme change; that keeps them cheap in large-file mode too. While the Mark row shows, the bar's own highlighting of every match is off, so the marks show what Mark All found. Jump Down and Jump Up to the Find Mark Style (Alt+M, Alt+Shift+M).
- **Style tokens** (Search › Style All Occurrences of Token › Using 1st–5th Style; Clear Style › 1st–5th, All Styles; Jump Down Ctrl+1–5, Jump Up Ctrl+Alt+1–5): the selection (one line) or the word at the caret (letters, digits and `_`, as a whole word), case-sensitively, in the whole document.
- **Split view** ([ADR-018](DECISIONS.md#adr-018--split-view-and-compare-presentation-m7-2026-10-01)): two views side by side, each an AdwTabView with its own strip, divided at one place; a view shows only while it has tabs, so one view looks as before. View › Move/Clone Current Document › Move to Other View and Clone to Other View; Focus on Another View (Ctrl+Alt+O, F8); the focus or a click on a strip makes a view active, and the active view is where commands act and new tabs open and what the title and status bar show (its tab is underlined in the accent colour). A clone is a second tab over the same document and buffer, with its own caret, scroll position and banners; one view of a buffer is in column mode at a time; operations over open documents count a document once; closing a clone never asks. A tab dragged onto the other view's strip moves there. Closing the second view's last tab collapses the split. View › Synchronise Vertical and Horizontal Scrolling. The session keeps each tab's view, clones (with their own caret), the tab in front of the other view and the divider ([ADR-006 amendment](DECISIONS.md#adr-006-amendment--split-view-clones-and-bookmarks-m7-2026-10-01)).
- **Compare** (Tools › Compare, Ctrl+Alt+C): the current document (new) against the other view's (old), or, without a split, the tab used before, which moves to the other view; Compare with File…, Compare with Clipboard (Ctrl+Alt+M) and Compare with Saved Version (Ctrl+Alt+D) open their text read-only in the other view, closed again by Clear Compare (Ctrl+Alt+X). `domain::diff::compare` runs on a worker; added, removed, changed and moved lines get paragraph backgrounds from the theme (the view paints empty lines, where GTK paints none), the characters that differ in a changed line a stronger one, and padding keeps equal lines level (`pixels-below-lines`, or the view's top and bottom margin, painted by the view); word wrap is off and the views scroll level while compared. Next and Previous Difference (Alt+PgDn, Alt+PgUp) wrap around; Ignore Whitespace and Ignore Case compare again; an edit compares again 300 ms later; the status bar and a toast give the summary. The changed-on-disk banner (M3) and the restored-conflict banner (M2) have a Compare button: the buffer against the file on disk.
- **Registry:** 51 new actions (208 in all); the menus' new places are in [DESIGN.md](DESIGN.md#as-built-in-m7--2026-10-01) and the keys in the [ADR-009 amendment](DECISIONS.md#adr-009-amendment--marking-view-and-compare-keys-m7-2026-10-01). The user guide's keymap table is regenerated.
- **Domain:** `marks::history` (the steps that keep bookmarks and marks across Undo and Redo, with unit tests and a property test that replays steps as GTK does), `CompareStats::summary`, the session fields, the theme's mark, bookmark and comparison styles; `marks` (bookmarks, line operations, styles) and `diff` were in the domain before M7 began.
- **Self-tests** in CI: `bookmarks`, `marks`, `marks-large`, `tokens`, `split`, `split-session` (children in `tests/selftest/session/`), `compare`, `compare-banner`, `compare-large`, the restored-conflict banner's Compare in `session/after-damage`, and the M7 colours before and after a theme switch in `theme`; by hand: `perf-compare`, `perf-marks-large-file`, `preview-m7`.

**Checks** ([TESTING.md](TESTING.md#2026-10-01--m7-marking-split-view-and-compare)):
- `STET_CI_DISPLAY=61 bash tools/ci.sh --ui` passed on the final code, `ba642fd`: fmt, Clippy `-D warnings`, 698 tests, the build, the smoke run and 4,442 self-test steps in 47 scripts (`bookmarks` 118, `marks` 103, `marks-large` 36, `tokens` 42, `split` 124, `split-session` 6, `compare` 128, `compare-banner` 40, `compare-large` 25 and `theme` 82 among them). The five full runs before it that day passed too.
- **Exit criterion, "marking a regex with bookmark lines, then removing the unbookmarked lines, is one undo step":** `marks.stet-test` on 6 lines, and `marks-large.stet-test` on 105,613 lines (1.1 MB) in CI and by hand: in the release build Mark found the regex's 10,562 matches on a worker and bookmarked their lines in 73.9 ms, Remove Non-Bookmarked Lines computed its 10,563 deletions on a worker and applied them as one bulk edit in 259.9 ms, `assert-undo-steps` counted one undo step, one Undo brought back the lines with their 10,562 bookmarks and the marks, and Redo removed them again with the bookmarks; the main loop's longest block was 253.6 ms (debug build: 286.5 ms; bar 500 ms). In large-file mode (`perf-marks-large-file`, a 60 MiB log, release build) Mark bookmarked 11,698 lines in 162.5 ms with the main loop never blocked over 63.3 ms, and Remove Non-Bookmarked Lines was one undo step (561.3 ms, longest block 442.9 ms); its Undo is in the gaps.
- **Exit criterion, "comparing two 10k-line files takes < 1 s":** `perf-compare.stet-test`, release build, three runs: two generated texts of 11,878 and 11,860 lines (105 added, 123 removed, 235 changed, 6 moved) compared end to end (snapshots, the diff on a worker, tags and padding in both documents) in 7.6–8.3 ms right after opening them, 44.5–47.8 ms with both laid out in one view (Compare moves one to the other view; the diff itself takes about 3 ms, the rest is its result waiting for the main loop) and 11.0–11.7 ms again after an edit; after a Replace All that changed 11,754 lines, 64.0–67.4 ms, and after upper-casing every line, 25.9–26.3 ms. The first painted frame followed 44.3–99.0 ms after the command, except right after opening (see the gaps). All 11,752 rows with a line on both sides were level to the pixel; the main loop never blocked over 51.5 ms. The debug build took 48.9–53.7 ms in the first three cases and 434.3 and 326.1 ms in the last two.
- **Rendered colours:** `assert-compare-paint` and `assert-text-background` read the pixels the views render: each kind of compared line, empty lines included; the marks in the open Mark row; marks, a style token, smart highlighting and compared lines before and after a theme switch (`theme.stet-test`). In three control runs, each with one of the fixes these checks came with undone, the checks failed.
- **Domain:** the `marks`, `diff` and `session` tests with `PROPTEST_CASES=5000`: 80 pass (4 timing tests ignored); the history's new property test replays random steps as GTK does, and caught a deliberately wrong footprint. `diff`'s timing tests in a release build: 2.5 ms for 10,000 lines with 499 hunks, 34.5 ms for 10,000 changed lines, 338.7 ms for 100,000.
- **Preview:** `preview-m7.stet-test` (36/36) wrote the three screenshots below with Omarchy's installed themes.

**Decisions:**
- **Keys** ([ADR-009 amendment](DECISIONS.md#adr-009-amendment--marking-view-and-compare-keys-m7-2026-10-01)): F-key-free keys first: Ctrl+Alt+K, L and J for the bookmarks (the Bookmarks extension for VS Code) before the familiar Ctrl+F2, F2 and Shift+F2; Ctrl+Alt+O before F8. The familiar Ctrl+M and Ctrl+1–5; Jump Up is Ctrl+Alt+1–5, because GTK can't match Ctrl+Shift with a digit (Shift turns it into a symbol that depends on the layout); the Find Mark Style jumps are Alt+M and Alt+Shift+M, since Ctrl+0 stays Reset Zoom. The jumps are editor keys, so Alt+M never takes a dialog's mnemonic. Familiar keys for Compare.
- **Bookmarks across Undo and Redo** are restored by recognising Stet's own steps in what GTK replays (their footprint), without a hook into GtkTextHistory (ADR-003 amendment). Marks are kept the same way.
- **Persistence:** bookmarks, views, clones and the divider are kept; marks, style tokens, the comparison and the scroll-sync toggles are not.
- **Split and compare presentation** (ADR-018): two AdwTabViews with their own strips, shown only when they have tabs; a clone shares the buffer; a comparison colours and pads the documents themselves, with the document in front as the new version.

**Findings:**
- GtkTextBuffer keeps a line's marks in a list that removing a mark walks from its head, so GtkSourceView's `remove_source_marks` took 1.06 s for the 10,562 bookmarks a replacement had collapsed onto one place; removing them in the list's order takes 0.14 s (release build).
- GTK paints a `paragraph-background` only on lines with characters (`gtk_text_layout_snapshot` skips an empty line's paragraph), so empty added, removed and moved lines showed no colour; the view paints them now, and `assert-compare-paint` reads the rendered pixels (without the fix the two empty lines in `compare.stet-test` came out `#ffffff`).
- GtkSourceView's gutter renderers centre a line's number and mark across its height, which the comparison's padding stretches; they sit at the top of the line now.
- Smart highlighting (M4) showed the scheme's `search-match` colour instead of `stet:smart-highlight` until the first theme switch, and after a switch the previous theme's colour (dark boxes around dark text in a light theme): GtkSourceView 5.20's `set_match_style` applies a style only at the buffer's next scheme or highlighting change. Stet turns the highlighting off and on after setting it; `theme.stet-test` checks the rendered colour before and after a switch, and both checks failed without the fix.
- Toasts were read as markup (libadwaita's default), so the one with `<Control>d` (M5's keys.toml error) logged a GTK warning ("Failed to set text … from markup"), in the CI logs and with the main checkout's build too, and a file name with `&` in M7's Compare messages would have done the same; toasts are plain text now.
- In a changed block too large for a full search (over 65,536 line combinations), `domain::diff` pairs lines only near the block's diagonal (16 lines), so when lines were also added or removed inside it, many changed lines show as removed and added instead: in `perf-compare`'s fourth case 3,877 of the 11,754 lines changed by a Replace All paired.

**Known gaps:**
- GTK draws a buffer's selection and current line in every view of it, so the other view of a cloned document shows the active view's selection; each view's own caret comes back when it becomes active.
- Synchronised scrolling keeps the pixel distance between the views; it doesn't follow lines when the two documents' lines differ in height.
- Compare refuses documents in large-file mode and files with lines over the long-line limit; it compares whole documents (no selection compare) and isn't kept in the session. Compare with File… opens a read-only copy, not the file itself.
- The first frame after a Compare made right after opening two 12,000-line files came late: 646.2, 1,138.9 and 1,170.4 ms after the command in the final release runs (381.5–1,334.9 ms over the twelve release runs that day), while the main loop never blocked over 52 ms; with the documents laid out it came after 83–89 ms. GTK was still measuring the 23,738 lines just opened; whether Broadway's frame pacing adds to it was not settled (live check 5).
- Undoing a bookmarked-line operation that removed most of a file in large-file mode is GTK's one-step undo of a large edit (M5's known P3): on the 60 MiB log the main loop blocked 6.7 s in the release build and 6.4 s in the debug build while GTK put the text back; the bookmarks came back.
- The style tokens are case-sensitive, and the caret's word is matched as a whole word; there are no options for them yet.
- Marks and style tokens keep the text's own colour, so dim text on them (comments in tokyo-night: line 5 of the first screenshot) has little contrast.
- An undo of a step that GTK can't tell apart from one of Stet's (the same footprint at the same place) would get its bookmarks.
- What an older build makes of a session with clones was not checked.
- The gutter click, real key presses, real focus and scrolling with a wheel in two views are only simulated headless; see the live checks.

**Preview gate:** [bookmarks, Mark and style tokens, tokyo-night](../tools/progress/public/m7-bookmarks-marks-tokyo-night-2026-10-01.png), [split view with a clone, catppuccin-latte](../tools/progress/public/m7-split-clone-catppuccin-latte-2026-10-01.png), [a comparison, gruvbox](../tools/progress/public/m7-compare-gruvbox-2026-10-01.png); judged after the live checks below.

**Still to check in the live Hyprland session** (release build, Norwegian layout with fcitx5):
1. **Keys:** Ctrl+Alt+K toggles a bookmark, Ctrl+Alt+L and Ctrl+Alt+J go to the next and previous one (and F2, Shift+F2, Ctrl+F2 with Fn); Ctrl+M opens the Mark row with the focus in the field; Ctrl+1 to Ctrl+5 and Ctrl+Alt+1 to Ctrl+Alt+5 jump through styled text (check that Ctrl+Alt with a digit reaches the app and AltGr+digits still type `@`, `£`, `$`, `€` in the editor); Alt+M and Alt+Shift+M jump through marks, and Alt+M in a dialog still takes its mnemonic; Ctrl+Alt+O switches views (F8 with Fn); Ctrl+Alt+C, Ctrl+Alt+M, Ctrl+Alt+D, Ctrl+Alt+X and Alt+PgUp/Alt+PgDn in a comparison.
2. **Gutter with the real mouse:** a click left of the line numbers toggles a bookmark on that line, at any zoom; the disc shows in tokyo-night, catppuccin-latte and gruvbox, and follows a theme change.
3. **Split with real focus:** Move to Other View and Clone to Other View; clicking into each view makes it active (title, status bar, the underlined tab); typing goes where the caret is; a click on the other view's tab strip; dragging a tab onto the other strip with the mouse; closing the last tab of the second view; the divider dragged with the mouse, and back after a restart.
4. **Synchronised scrolling** with a touchpad and a mouse wheel, vertical and horizontal, on long files; scrolling a comparison keeps equal lines level.
5. **Compare in live themes:** two versions of a real file (`git show HEAD~5:path > /tmp/old.rs`): the colours (empty lines included) are legible in tokyo-night, catppuccin-latte and gruvbox; padding lines up equal lines; Next and Previous Difference; Compare with Clipboard after copying text from another app; Compare on the changed-on-disk banner after `echo x >> file` from a terminal.
6. **Mark** on a large log (a regex with Bookmark line, then Remove Non-Bookmarked Lines, then Ctrl+Z once): the window keeps painting and one undo brings everything back (over 50 MiB that undo holds the window for seconds; see the gaps).

**Follow-ups:**
- P0 (M7 exit): the live checks above, and the preview gate.
- P2: AGENTS.md's "Current scope" and README's status paragraph still describe the state before M2; update them with the next merge.
- P2: options for the style tokens (match case, whole word), and "Style One Token" and "Copy Styled Text" per style.
- P3: pair changed lines in large blocks with added or removed lines among them (a windowed search around the diagonal's drift).
- P3: synchronised scrolling by lines; compare selections; keep the comparison in the session.
- P3: `indent.stet-test` logs `Gtk-CRITICAL: gtk_adjustment_set_value: assertion 'isfinite (value)' failed` when the document map (M5) is shown again; the main checkout's build (without M7) logs it too. Not investigated.

## M2, M5, M6 and M8 merged; MVP tagged; live acceptance — 2026-10-01

Merged on `main` under the standing authorization of 2026-10-01, each with `git merge --no-ff`, CI after each, then the live acceptance script in the user's Hyprland session. M7 is being built in its own worktree from `123381c`.

**Merges and CI** (`STET_CI_DISPLAY=51 bash tools/ci.sh --ui`, all green; [TESTING.md](TESTING.md)):
- M2 (`53cd1e1`): 598 tests, 1,801 self-test steps in 20 scripts (after the fixes below: 597 tests, `d047a9f`).
- M5 (`6dd83f0`): 639 tests, 2,507 steps in 27 scripts.
- M6 (`3be34a6`): 3,151 steps in 31 scripts.
- M8 (`123381c`): 687 tests, 3,786 steps in 38 scripts.

**Integration work at the merges:**
- The first window waits for the settings files (M5), then the restored session (M2); the self-test harness has every milestone's steps; the registry has 157 actions and the user guide's keymap table is regenerated from it at each merge; the license report is regenerated (130 crates after M2 and M5).
- M2's backups follow M5's `config.toml`: `backup_interval` sets the timer; `backups = false` writes no unsaved text anywhere and closing the window asks again ([ADR-006 amendment](DECISIONS.md#adr-006-amendment--the-backup-settings-merge-of-m2-and-m5-2026-10-01), `session-settings.stet-test`).
- M5's whole-line Copy and Cut step aside while an M6 rectangle is active; M6's timing scripts pin tabs four wide since M5 detects a file's indentation.
- M8's Pin Tab and Unpin Tab joined M5's tab menu (M8's own menu replaced it otherwise); Close Others and Close to the Right skip pinned tabs; brace jumps are places for Go Back; colliding self-test steps renamed.
- Stet finds Omarchy's state under `$HOME/.local/state/omarchy`, where Omarchy's scripts keep it whatever `$XDG_STATE_HOME` says (`6a25be7`), which also settles M5's default-editor path note; the CI smoke run uses scratch state, so it no longer writes the user's session.

**Found live and fixed:**
- Since M2 the first window waited for the whole load of a command-line file: a 94 MiB file's window appeared after 1.7 s and registered for accessibility only after GtkTextView's validation, 44 s later ([ADR-003 amendment](DECISIONS.md#adr-003-amendment--the-first-screen-of-a-load-after-m2-2026-10-01), `d047a9f`): now its first screen is painted 545–706 ms after the process starts.
- A dialog opened from a popover menu lost its focus widget when the popover closed; Rename File… opened with the focus on its button (`a4cc00c`).

**Live acceptance** ([TESTING.md](TESTING.md#2026-10-01--live-acceptance-on-hyprland-after-merging-m2-m5-m6-and-m8)): 30 checks, all passing (quick open after a fix to the check), plus the opt-in D-Bus service check. They cover, live: sessions through SIGTERM, `kill -9` and `systemctl --user stop`; `GIT_EDITOR` with Stet running and not; M5's keys on the Norwegian layout and the tab menu with the real mouse; column mode's keys, Column Editor and clipboard; the AT-SPI crash fix; the Ctrl+Tab switcher with real key releases; back and forward with keys and mouse buttons; quick open; Replace in Files on a scratch git tree.

**Tags:** `v0.2.0` on `1a60210` (Wave A: M1, M3 and M4, search) and `v0.3.0` on `6dd83f0` (M5 merged: the MVP), as annotated tags; the Cargo version stayed 0.0.1 at both, so they mark milestones, not packages. The version becomes 1.0.0 in M9.

**Still for the user's own hands:** a real logout and reboot with drafts open (rows 3 and 22), `SUDO_EDITOR="stet --wait" sudoedit` (row 23, needs the sudo password), Alt+drag with the real mouse and fcitx5 preedit in column mode (row 24), fractional scaling (row 8), CJK and compose input (rows 9, 9a), "Set as default editor" for the real session (row 4), and a Nautilus double-click (row 5).

**Follow-ups for M9:**
- P2: startup median 350 ms against 400 ms (284 ms before M5); find what M5, M6 and M8 added to the first frame.
- P2: accessibility: GTK 4.22's popover menu items, the find bar's fields and the Column Editor's radio buttons (names with the mnemonic underscore, no action) need names or labels for Orca.
- P3: undo of a large tool result or Replace All is one GTK step applied at once (0.6 s for 20 MB; 1.5 s for 1M replacements).
- P3: GtkTextView validates every line after a big load (44 s of a busy main thread for 1M lines); idle work waits meanwhile.

## M8 built and checked headless; live checks pending — 2026-10-01

Built in the M8 worktree from `53cd1e1` (`main` with M2 merged) under the standing authorization of 2026-10-01, while M5 (text toolbox and chrome) and M6 (column mode) were built in parallel worktrees; nothing of theirs is in this work. All evidence is headless (GTK Broadway, displays 91–93); what needs the live session is at the end.

**What exists:**
- **Quick open** (Ctrl+P; File › Quick Open…; the palette): the palette's popover at the top centre with a field, up to 50 rows and a note. Rows are the open documents (most recently used first), the recent files, then the project's files: the project is the nearest folder up from the current document's that holds `.git`, else that folder, and its files come from a walk on a worker (`infrastructure::project::walk_project`: `.gitignore` respected, hidden files and binary-looking extensions left out, at most 100,000 files, streamed in batches). Ranking (`domain::fuzzy`, file-name hits first; `domain::quick_open`) runs on a thread of its own over shared lists; a newer query drops older rankings, and while files stream in it ranks again at most every 60 ms. Matched characters are bold. `name:line[:col]` opens at a line and `:line` goes to a line of the current document. Enter opens the file or switches to its tab, Escape closes, and the editor gets the focus back.
- **Ctrl+Tab switcher** (Next Recent Tab, Previous Recent Tab; [ADR-009 amendment](DECISIONS.md#adr-009-amendment--navigation-keys-m8-2026-10-01)): the tabs in most-recently-used order (`domain::nav::Mru`). With Ctrl held, a card shows the order after 120 ms; Tab and Shift+Tab move, releasing Ctrl switches, Escape cancels, Enter switches at once. A quick tap goes to the previous tab without the card. Ctrl+PgDn and Ctrl+PgUp keep the strip's order.
- **Back and forward** (Alt+Left, Alt+Right, Search › Go Back and Go Forward, the palette, the mouse's buttons 8 and 9): jumps record the place they leave (`domain::nav::History`): a tab switch, Go to Line, Find Next and Previous, opening the find bar, Next and Previous Search Result and the results panel, quick open, opening a file at a line (`Window::note_jump`). Places within 10 lines merge, a new jump cuts the places ahead, edits move places along, closing a tab drops its places, and going back to a restored tab that hasn't loaded waits for its text.
- **Pinned tabs** (the tab strip's context menu, the View menu, the palette): AdwTabView's pinned pages, icon-only at the start of the strip (the file type's icon, a dot while dirty). Close All skips them; closing a pinned tab itself works and asks when it has unsaved changes. The session keeps them, stubs included ([ADR-006 amendment](DECISIONS.md#adr-006-amendment--pinned-tabs-and-first-line-names-m8-2026-10-01)).
- **First-line names** (`domain::untitled`): an untitled tab is named after its first line with text (white space folded, 30 characters and an ellipsis), else "Untitled N". The window title, the switcher, quick open, the close prompt and the session use it; Save As proposes it as a file name with `.txt` or the language's extension.
- **Replace in Files** (the files row's Replace in Files button, the palette; [ADR-005 amendment](DECISIONS.md#adr-005-amendment--replace-in-files-m8-2026-10-01)): in Find in Files mode the bar shows the replace field too. After a confirmation that names the pattern, the replacement, the files, the folder and the walk's options, and says that files on disk can't be undone, `infrastructure::replace_in_files` walks as Find in Files does on worker threads and replaces with Replace All's engine in each file's decoded, LF-normalized text, writing only the replacements in the file's own encoding and line ending and copying every other byte; `safe_save` keeps links, mode and owner, and the file's fingerprint is checked right before. Files that match but are binary, undecodable, unrepresentable, read-only, over 50 MiB, changed meanwhile, or would have a CR and an LF joined are left alone and listed with the reason; files without a match are never listed. Open documents, restored tabs that haven't loaded included, get the change in their buffer as one undoable edit (bulk above 2,000 edits); a clean one is then saved when its save writes its bytes back exactly, a dirty one stays unsaved. The results panel lists every file, its outcome and its replaced lines (at most 50,000 lines a run); Stop ends the walk, a file is written whole or not at all, and the files written are listed.
- **Registry:** 76 actions (M2's 68 and Quick Open, Replace in Files, Go Back, Go Forward, Next Recent Tab, Previous Recent Tab, Pin Tab, Unpin Tab). Next Tab and Previous Tab keep only Ctrl+PgDn and Ctrl+PgUp.
- **Self-tests** in CI: `switcher`, `back-forward`, `quick-open`, `pinned`, `first-line`, `replace-in-files` and `rif-fixtures` (`pinned` and `replace-in-files` with children in `tests/selftest/session/`); by hand: `perf-quick-open`, `perf-replace-in-files`, `preview-m8`.

**Checks** ([TESTING.md](TESTING.md#2026-10-01--m8-navigation-and-replace-in-files)):
- `STET_CI_DISPLAY=91 bash tools/ci.sh --ui` passed on the final code, `aab20a7`: fmt, Clippy `-D warnings`, 639 tests, the build, the smoke run and 2,436 self-test steps in 27 scripts (`switcher` 93, `back-forward` 88, `quick-open` 89, `pinned` 69, `first-line` 46, `replace-in-files` 116, `rif-fixtures` 134 among them). It also passed on the working tree just before.
- **Exit criterion, "Replace in Files keeps each file's encoding, BOM and EOL on all fixtures":** `rif-fixtures.stet-test` writes the fixture corpus (32 files: UTF-8 ± BOM, UTF-16 LE/BE ± BOM with NUL and a surrogate pair, Windows-1252, ISO-8859-15, Windows-1251, Shift_JIS, GBK, GB18030, Big5, EUC-KR, ISO-2022-JP, CR-only, CRLF, a CRLF file of 30 lines, mixed, empty, NUL, binary, lossy and broken UTF-8) to a temporary folder and runs one Replace in Files of the regex `^` with `>\n`: the 23 text files are byte for byte what a byte-level oracle computed without any text codec (70 replacements, each line break in the file's dominant ending, after the BOM), each reopens with the same encoding and line ending in the status bar, and the 9 binary and lossy files are untouched and listed with their reasons. The unit test `every_fixture_keeps_its_encoding_bom_and_line_endings` replaces the first word of every fixture with a line break against independently encoded bytes; its proptest companion ran 5,000 cases.
- **Quick open** (release build, `perf-quick-open.stet-test`): over 50,000 generated files the first rows showed 0.9 ms after opening and the first project files after 15.1 ms (the walk 12.4 ms); a typed query over all 50,000 reached the screen in 2.3–2.8 ms. Over `~/Projects` (42,532 files, read-only) the walk took 13.3 ms and typed queries 7.8 and 13.4 ms. Warm caches; a cold start was not measured.
- **Replace in Files** (release build, `perf-replace-in-files.stet-test`, 10,000 files of 20 lines on tmpfs): 200,000 replacements in 114.7 ms on the worker and 1,965 ms until the panel had listed the last file; a run that changes nothing 54.3 ms; Stop part-way finished 38.4 ms after it was pressed.

**Decisions:**
- Keys ([ADR-009 amendment](DECISIONS.md#adr-009-amendment--navigation-keys-m8-2026-10-01)): Ctrl+P quick open (Print, Ctrl+P by a common convention, is 1.x), Ctrl+Tab and Ctrl+Shift+Tab the switcher, Ctrl+PgDn and Ctrl+PgUp the strip's order, Alt+Left and Alt+Right back and forward, plus the mouse's buttons 8 and 9.
- **How Ctrl+Tab and Alt+Left/Right were taken over:** AdwTabView's own shortcuts have been off since M1, and the keys are registry accelerators; GTK runs application accelerators in the window's shortcut controller in the capture phase, before any widget inside the window, so neither the tab view, GtkSourceView's `move-words`, GtkWindow's focus keys nor an editor-scoped capture controller (M5's) sees them first. A key controller on the window, also in the capture phase, sees Ctrl released (and Escape and Enter while the card shows) and consumes nothing else; the mouse's buttons go to a click gesture on the window in the capture phase.
- **Open documents in Replace in Files** ([ADR-005 amendment](DECISIONS.md#adr-005-amendment--replace-in-files-m8-2026-10-01)): the buffer gets one undoable edit; a clean document is saved (unless its save would change other bytes or its file changed on disk), a dirty one stays unsaved, and its file is never written by the run.

**Deviations and findings:**
- Files that match nothing are not listed, binary ones included; skipped files say how many matches they had.
- Listing 200,000 replaced lines took 6.65 s in a first version, while the files were written in 121 ms; the panel now lists at most 50,000 lines a run (Find in Files' hit cap) and the rest of the files with their counts, and after Stop it lists the files written so far at once (Stop's end went from 919 ms to 38 ms).
- An open document closed during a run was neither changed nor listed; it is listed as skipped now ("the document was closed while replacing"), as is one that doesn't finish loading within 30 s.
- First-line names changed three existing expectations: `files.stet-test` (a restored untitled tab) and the `after-kill` and `after-damage` session children.
- A closed ISO-2022-JP file is skipped when a replacement would cut an escaped run (its other bytes would change); an open one is saved by M3's save path like any edit.
- **Merging with M5** (built in parallel; these files will conflict): M5's tab context menu should take Pin Tab and Unpin Tab (only the enabled one shows) and replace this work's two-item menu (`app/src/window/pins.rs`, keeping `menu_page` so tab commands act on the clicked tab); M5's Close Others and Close to the Right must skip pinned tabs through `Window::closable_by_bulk_commands`, as Close All does; Ctrl+Tab and Ctrl+Shift+Tab belong to `NextRecentTab` and `PreviousRecentTab` (the registry's unit tests fail if M5 keeps them on `NextTab` and `PreviousTab`); M5's brace jump should call `Window::note_jump()` first so Go Back returns from it; M5's editor-scoped capture controller that swallows Alt+Left and Alt+Right is compatible, because the accelerators run first in the window's capture phase; the new actions use `command(…)` and so take whatever default key scope M5 gives it.

**Preview (headless):** [quick open, tokyo-night](../tools/progress/public/m8-quick-open-tokyo-night-2026-10-01.png), [switcher with a pinned tab and a first-line name, catppuccin-latte](../tools/progress/public/m8-switcher-pinned-catppuccin-latte-2026-10-01.png), [Replace in Files' confirmation, gruvbox](../tools/progress/public/m8-replace-in-files-confirm-gruvbox-2026-10-01.png), [its results, gruvbox](../tools/progress/public/m8-replace-in-files-results-gruvbox-2026-10-01.png). Judged by the agent: legible in a dark, a light and a warm theme; the pinned tab shows its icon at the start of the strip, the untitled tab its first line, the card and the popover the theme's popover colours, and the results a skipped Windows-1252 file with its reason next to replaced and saved open documents. The gate stands after the live checks below.

**Still to check in the live Hyprland session** (release build, Norwegian layout with fcitx5):
1. **Ctrl+Tab with real keys**, four or more tabs: a quick Ctrl+Tab goes to the previous tab with no card flashing; holding Ctrl and pressing Tab twice shows the card after about 120 ms and moves the selection in most-recently-used order; releasing Ctrl (left and right Ctrl) switches; Ctrl+Shift+Tab moves back; Escape while holding cancels; Enter switches at once; Ctrl+PgDn and Ctrl+PgUp go in strip order; from the find field Ctrl+Tab switches and the typing stays in the field.
2. **Alt+Left and Alt+Right** in the editor go back and forward (Ctrl+G to a far line, then back) and no longer move by words; with a mouse that has side buttons, buttons 8 and 9 do the same, in the editor and over the tab strip.
3. **Quick open on `~/Projects`:** open a file in a repository under `~/Projects` and press Ctrl+P (it reaches the app): rows at once, the project's files streaming in, ranking instant while typing, `target/` and other ignored files absent, `name:40` opening at line 40, Escape back to the editor; the same with a cold cache after a reboot.
4. **Replace in Files on a scratch copy of a real tree** (never the original): `git clone ~/Projects/linux-apps/hertz /tmp/rif-check` (or a copy with mixed encodings), open one file clean and another with unsaved edits, replace a common identifier: `git -C /tmp/rif-check diff --stat` and `git diff` show only the replaced lines (no line-ending or encoding changes), the clean open file is saved and Undo takes the edit back, the dirty one stays unsaved; then Stop part-way through a larger run.
5. A real right-click on a tab offers Pin Tab or Unpin Tab; pinned tabs and the switcher in the live themes; the window title of an untitled tab in Hyprland's window switching.

**Follow-ups:**
- P0 (M8 exit): the live checks above.
- P1 (merge with M5): the five points under "Merging with M5"; AGENTS.md's "Current scope" and README's status paragraph are left for that merge.
- P2: quick open walks the project every time it opens (13 ms over 42,532 files warm); cache the list per project if cold walks of large trees prove slow.
- P2: the back-and-forward places and the most-recently-used order are not kept in the session.
- P3: Replace in Files has no preview.
- P3: a closed ISO-2022-JP file with a replacement inside an escaped run is skipped instead of re-encoded.

## M6 Column mode built and checked headless; live checks pending — 2026-10-01

Built in the M6 worktree from `7387fb0` (`main`: Wave A and the live acceptance fixes) under the standing authorization of 2026-10-01, while M2 and M5 were built in other worktrees. The toolkit-free column core (`stet_domain::column`) already existed; M6 adds the GTK side, two domain helpers and the docs. All evidence is headless (GTK Broadway); what needs the live session is at the end.

**What exists:**
- **`StetView`** (`app/src/column/`), the GtkSourceView subclass every tab's editor now is. It keeps the rectangle (anchor and cursor corner in visual columns, virtual space allowed) and paints it below the text in the theme's selection and caret colours, visible rows only ([DESIGN.md](DESIGN.md#as-built-in-m6--2026-10-01)).
- **Entering and leaving column mode:** Alt+Shift+arrows, Home, End, PgUp and PgDn (and the keypad's), Begin/End Select in Column Mode (Alt+Shift+B), Alt+drag, Alt+click, Alt+Shift+click. Escape, plain arrows and a click leave it at the cursor corner; actions that don't know columns (Find, Select All, Go to Line, …) leave the two corners selected; edits and caret moves made by anything else end it ([ADR-016 amendment](DECISIONS.md#adr-016-amendment--as-built-in-m6-2026-10-01)).
- **Alt+Shift+arrows** are registry actions with application accelerators, which GTK runs before GtkSourceView's `move-viewport` bindings; nothing in GtkSourceView is unbound ([ADR-009 amendment](DECISIONS.md#adr-009-amendment--column-mode-keys-m6-2026-10-01)).
- **Column editing** through `domain::column`: typed text (GtkTextView's commits, so plain keys, compose, dead keys and fcitx5 alike) goes in on every line, padding short lines; typing over a block replaces it; Backspace, Delete and Tab; overwrite mode types over as many cells as it types; Enter leaves column mode. A typing burst is one undo step, closed before anything else acts: a window controller ahead of the accelerators (chords, navigation keys, Escape, clicks), every registry action, the focus leaving the view, the end of column mode, 1 s without a key.
- **Large rectangles** (over 2,000 edits) are one undo step made of single edits near the view and at most two replacements of line ranges, so the view doesn't move; source marks are restored by line ([ADR-003 amendment](DECISIONS.md#adr-003-amendment--large-column-edits-keep-the-view-m6-2026-10-01)).
- **Rectangular clipboard:** `application/x-stet-column-block` and plain text; a block pastes as a rectangle in column mode and at a normal caret; plain text into a rectangle: one line goes in on every row, several lines go in as a block.
- **Column Editor** (Alt+C, Edit menu, palette): text or numbers (initial, increase by, repeat, leading none, zeros or spaces, Dec, Hex with uppercase, Oct, Bin), on the rectangle's lines or from the caret to the end of the text; one undo step; errors in the dialog; the focus back in the editor.
- **Status bar:** `Ln, Col` at the cursor corner, virtual columns included, and `Sel rows×columns`.
- **Accessibility:** a NULL-safe `GtkAccessibleText.get_selection`, so AT-SPI `AddSelection` no longer crashes Stet on GTK 4.22 ([upstream note](upstream/gtk-accessible-text-add-selection.md)).
- **Undo after Redo:** GTK 4.22 joins the next edit to a step that was undone and redone; every editor buffer works around it ([upstream note](upstream/gtk-text-history-redo-barrier.md)).
- **Registry:** 10 new actions (77 in all); the eight Column Select actions are in the palette only.
- **Domain:** `Block::from_plain_text` and `home_column`, each with a unit test and a proptest.
- **Self-tests** `column`, `column-clipboard`, `column-editor` and `column-large` in CI; `perf-column` and `preview-column` by hand ([TESTING.md](TESTING.md#ui-self-tests-stet---self-test-from-m1)).

**Checks** ([TESTING.md](TESTING.md#2026-10-01--m6-column-mode-ci-self-tests-and-the-10000-line-timings)): `STET_CI_DISPLAY=81 bash tools/ci.sh --ui` passed on the final code: fmt, clippy `-D warnings`, 598 tests, the build, the smoke run and 2,228 self-test steps in 21 scripts (`column` 331, `column-editor` 174, `column-clipboard` 84 and `column-large` 50 among them). The exit criteria, headless, on 10,000 lines of S5's shapes:
- Inserting at column 0 on every line is one undo step: 14.1–15.2 ms with the release build and the view in the middle (three runs; bar 200 ms), 12.0–12.4 ms with the view at the top; 29.2–31.9 ms in debug builds.
- Its undo and redo: 24.7–25.8 ms and 26.2–27.7 ms (release, three runs; bar 200 ms each), the main loop blocked at most 33.1 ms over both; debug 22.8–31.8 ms and 23.9–34.2 ms.
- One key and Backspace on a 10,000-line caret 12.8–14.5 ms, a 10,000-row paste 14.5–16.9 ms (release).
- A typing burst undoes in one step, and each trigger that closes it is checked; the column-math proptests pass.
- fcitx5: the self-tests drive the path its commits take (`insert-text` from GtkTextView's commit); real fcitx5 is on the live list.

**Preview (headless):** [block, tokyo-night](../tools/progress/public/m6-block-tokyo-night-2026-10-01.png), [column caret, catppuccin-latte](../tools/progress/public/m6-caret-catppuccin-latte-2026-10-01.png), [Column Editor, gruvbox](../tools/progress/public/m6-column-editor-gruvbox-2026-10-01.png), [numbers, gruvbox](../tools/progress/public/m6-numbers-gruvbox-2026-10-01.png). The gate is judged after the live checks below.

**Decisions** (in the ADR amendments above): a tab across the column is split into spaces, not snapped; wide characters and combining marks are one cell each, as GtkSourceView's visual column counts; typing over a block replaces it; overwrite mode types over cells; Enter leaves column mode and breaks one line; plain text pastes into a rectangle with one line on every row, or several lines as a block; Alt+Shift+arrows are application accelerators; large edits keep single edits near the view; the GTK redo workaround applies to every editor buffer.

**Known gaps:**
- Undo and redo of a bulk column edit collapse the source marks on its replaced lines (S5); the edit itself restores them. M7's bookmarks have to restore them after undo and redo.
- With word wrap on, painting a rectangle that crosses a line's wrap point is not handled (the edits go by columns and are right); not tested.
- The pointer can't put the column between a letter and its combining mark; the keyboard can.
- Assistive technologies see the caret at the cursor corner, not the rectangle; the primary selection is not set from a rectangle.
- An input method that deletes around the caret ends column mode first.
- A typing burst on a rectangle of more than 2,000 lines keeps the text of the replaced lines in the undo history once per key; not measured.
- Like every application accelerator, Alt+Shift+arrows and Alt+C act on the current editor while the focus is in the find bar or the results panel.
- The Column Editor keeps its values only while Stet runs.

**Still to check in the live Hyprland session** (release build):
1. On the Norwegian layout, Alt+Shift+Left, Right, Up, Down, Home, End, PgUp and PgDn make and grow a rectangle (the status bar shows `Sel N×M`), and Alt+Shift+B twice selects from the first corner to the caret.
2. Alt+drag with the real mouse across tabs and past line ends, Alt+click and Alt+Shift+click; Hyprland passes Alt+drag to the window (Omarchy moves windows with SUPER+drag).
3. Alt+C opens the Column Editor with the focus in its first field; Enter writes the column; Escape closes; typing goes into the editor afterwards.
4. fcitx5 in column mode: the preedit shows at the cursor corner, the commit goes in on every line, and the burst undoes in one step (with a CJK engine, if one is installed). Compose (`compose:caps`) é and ø in column mode.
5. SUPER+C, SUPER+X and SUPER+V with a rectangle: the block is copied, cut and pasted back as a block; pasted into another application it is plain text; plain text from another application pastes into a rectangle: one line goes in on every row, several lines go in as a block.
6. `Atspi.Text.add_selection` on the editor with a selection, from another process: Stet keeps running ([manual row 25](TESTING.md#manual-omarchy-acceptance)).
7. Real keys: type in column mode, then Ctrl+Z (one step back) and Ctrl+Y; Escape and an arrow leave column mode.
8. The rectangle, the caret bars and the Column Editor in tokyo-night, catppuccin-latte and gruvbox: legible; compare with the screenshots.

**Follow-ups:**
- P0 (M6 exit): the live checks above, and the preview gate.
- P1 (M7): restore bookmarks after undo and redo of a bulk column edit.
- P1 (M5): reconcile with M5's keymap work: the column keys are registry keys with application accelerators, and GtkSourceView's `move-viewport` stays shadowed by them; an editor-scoped controller for the other GtkSourceView bindings in ADR-009 doesn't need these keys.
- P2: M4's Replace All applies a large bulk edit as one replacement of the whole range, which moved the view in M6's first build; check whether Replace All moves it too.
- P3: paint wrapped lines in column mode; expose the rectangle to AT-SPI; set the primary selection from a rectangle; keep the Column Editor's values in the settings.

## M5 built and checked headless; live checks pending — 2026-10-01

Built in the M5 worktree from `main` at `a9349a4` (Wave A: M1, M3 and M4) under the standing authorization of 2026-10-01, while M2 (session, backups, quitting without prompts, D-Bus activation, `--wait`) was built in parallel in another worktree and is not on this branch. All evidence is headless (GTK Broadway); what needs the live session is at the end.

**What exists:**
- **Text toolbox** (`app/src/window/toolbox.rs` over `domain::ops`; [DESIGN.md](DESIGN.md#as-built-in-m5--2026-10-01), [USER_GUIDE.md](USER_GUIDE.md#text-tools)): Edit › Line Operations (Duplicate, Cut, Copy, Delete, Transpose, Move Up and Down Current Line, Join Lines, Insert Blank Line Above and Below, four line removals, Reverse Line Order, twelve sorts), Convert Case to (seven conversions), Comment/Uncomment (tokens from the language's GtkSourceView metadata), Blank Operations (three trims, TAB to Space, Space to TAB all or leading), Add Prefix/Suffix… and Insert Numbers… (dialogs; Enter applies; the values are remembered), Tools › JSON (Format, Minify, Validate) and XML (Format, Validate; an error moves the caret to it), Search › Go to and Select to Matching Brace, Select and Find Next and Previous. Ctrl+C and Ctrl+X without a selection copy and cut the caret's line. Each tool is one undo step: up to 512 KiB it runs on the GTK thread, beyond that on a worker with a revision check while the editor takes no edits; more than 2,000 edits become one bulk edit, and a result over 1 MiB goes in pieces ([ADR-003 amendment](DECISIONS.md#adr-003-amendment--text-tools-on-workers-large-results-in-pieces-m5-2026-10-01)). Read-only and loading documents refuse with a toast.
- **Keys** ([ADR-009 amendment](DECISIONS.md#adr-009-amendment--text-tool-keys-m5-2026-10-01)): the familiar keys for the tools, UPPERCASE on Alt+Shift+U, Select and Find on Ctrl+Alt+F and Ctrl+Alt+Shift+F before Ctrl+F3 and Ctrl+Shift+F3, JSON and XML on Ctrl+Alt+Shift+M, C, J, B and X. The 29 text-tool keys are editor-scoped: a capture-phase GtkShortcutController on each editor, filled from the keymap, runs them before GtkSourceView's and GtkTextView's bindings, so they never act in the find bar's fields; it also takes the 24 built-in bindings ADR-009 overrides (`change-number`, `move-lines`, `move-words`, `move-viewport`, Ctrl+/ and Ctrl+\) and does nothing with those no text tool uses (Alt+Up/Down stay Find Previous and Next: application accelerators run first).
- **Settings files** ([ADR-017](DECISIONS.md#adr-017--settings-in-configtoml-and-keystoml-2026-10-01)): `config.toml` (font and size, tab width, spaces, detection, wrap, whitespace, document map, smart highlighting, word count, the backup settings for M2, the highlighting limits, `[language.<id>]` indentation) and `keys.toml` (keys per action, `[]` to unbind), parsed with `toml`'s spanned tables. Mistakes and key conflicts are reported with line and column, and a file with any is not applied; both files are reloaded when they change; Settings › Open Settings and Open Keyboard Shortcuts create them with every default commented out. Help › Keyboard Shortcuts is libadwaita's shortcuts dialog with every command's current keys.
- **Context menus:** the tab menu (right-click a tab; [INTEGRATIONS.md](INTEGRATIONS.md#7-other-desktop-actions)): Close, Close Others, Close to the Right; Copy Full Path, File Name, Directory Path; Open Containing Folder (`xdg-open`) and Open Terminal Here (`xdg-terminal-exec --dir=`), both started with `setsid --fork uwsm-app --`; Rename File… (`renameat2` with `RENAME_NOREPLACE`); Move to Trash… (asks, then `gio::File::trash`). The file commands are in the File menu too, for the current tab. The editor's context menu adds Convert Case to, Comment/Uncomment, Line Operations, Select and Find Next and Go to Matching Brace to GtkTextView's own items.
- **Indentation** (`domain::indent`, `app/src/window/indentation.rs`): detected from the first 10,000 lines (at most 256 KiB) when a file opens, otherwise the settings for the language (Stet's defaults: Makefile and Go tabs; Python and Rust four spaces; YAML, JSON and XML two); the status-bar item (`Spaces: 4`, `Tab Width: 4`) with a popover: Spaces or Tabs, width 1–8, Detect from Content, Convert Indentation to Spaces or to Tabs. ADR-015's exact tab stops are re-applied after every width change.
- **Status bar** complete: `Ln 5, Col 1, Sel 36 | 2`, the document's lines and characters, the optional word count, the language, line-ending, encoding and indentation buttons, INS/OVR. The window title is M1's.
- **Document map:** GtkSourceMap beside the editor; View › Document Map; off by default (`document_map`); never in large-file mode.
- **Set as Default Editor…** (Settings menu and palette; [INTEGRATIONS.md](INTEGRATIONS.md#4-set-as-default-editor-built-in-m5)): looks first, then says what it will write (Omarchy's `defaults/editor`, and `xdg-mime default` for the desktop entry's 16 MIME types) with warnings when `stet` or the desktop entry is missing, asks, applies, and reports what changed.
- **Registry:** 71 new actions (138 in all), the Settings and Tools menus, submenus in a familiar order; the menu has 162 items, all from the registry.
- **Docs and packaging:** [INSTALL.md](INSTALL.md) and [USER_GUIDE.md](USER_GUIDE.md) (new; the guide's keymap table is generated from the registry and a unit test keeps it in step; the package installs both); INTEGRATIONS.md sections 4, 6, 7 and 7a; DESIGN.md's M5 entry; the ADR-003, ADR-009 and ADR-014 amendments and ADR-017. `toml` 1.1 (`std` and `parse` only; already locked as a build dependency of the GTK `-sys` crates) is the one new dependency. The license bundle (`tools/license-report.py`) had not been regenerated since M1 and lacked M3's and M4's crates; it now lists 128 crates, up from 87.

**Checks** ([TESTING.md](TESTING.md#2026-10-01--m5-text-toolbox-and-chrome-ci-self-tests-and-large-inputs)): `STET_CI_DISPLAY=71 bash tools/ci.sh --ui` passed on `21ce1bc`, the final code: fmt, clippy `-D warnings`, 633 tests, the build, the smoke run and 2,272 self-test steps in 23 scripts, among them the new `toolbox` (284), `toolbox-large` (63), `settings` (107), `context` (123), `indent` (75) and `default-editor` (31). Two earlier full runs, on `b58646a` and `282fe3d`, passed as well.
- **Large inputs** (`toolbox-large`, release build): the 20 MB JSON file formatted in 1.62 s in 43 pieces with the longest main-loop block 37.0 ms (the PRD's "doesn't block the UI"; the script's limit is 200 ms); an integer sort of 3 MB in 0.33 s (longest block 55.0 ms); 551,022 trims as one bulk edit in 0.19 s; 7,350 trims on a 40 KB document in 1.9 ms on the GTK thread. The debug build in the three CI runs: 3.1–3.2 s, 1.43–1.46 s and 0.40–0.41 s, with blocks of at most 59.6 ms.
- **Keys and menus:** the editor's controller holds the 29 text-tool keys and the 24 overridden built-in bindings, each of them a real GtkSourceView or GtkTextView binding; the tab view's own shortcuts stay off; the menu has 162 items, all from the registry; the user guide's keymap table matches the registry (unit test).
- **Sandbox:** after every run the user's `~/.config/mimeapps.list` (2026-09-30) and `~/.local/state/omarchy/defaults/editor` (`code`, 2026-09-18) were unchanged, and there was no trash on `/tmp`.

**Decisions** (recorded in [DECISIONS.md](DECISIONS.md)):
- **UPPERCASE is Alt+Shift+U**, not the familiar Ctrl+Shift+U, which fcitx5 takes along with Ctrl+Alt+Shift+U. With fcitx5's hotkey cleared, `uppercase = ["<Control><Shift>u", "<Alt><Shift>u"]` in `keys.toml` restores Ctrl+Shift+U ([INTEGRATIONS.md](INTEGRATIONS.md#6-fcitx5-key-grabs)).
- **Editor-scoped keys** for the text tools; the familiar Ctrl+T, Ctrl+B and Ctrl+Alt+B kept (Ctrl+B and Ctrl+Alt+B through GtkSourceView's own `move-to-matching-bracket`); Ctrl+Alt+F and Ctrl+Alt+Shift+F for Select and Find; familiar keys for JSON and XML.
- **Settings are two TOML files applied whole** (ADR-017): a file with a mistake changes nothing, and the toast names the first mistake's line and column.
- **Text tools on workers above 512 KiB, results in pieces above 1 MiB** (ADR-003 amendment).
- **Without a selection,** the line tools, case conversions (the word at the caret) and comments act on the caret's line, the sorts, removals, trims and JSON/XML tools on the whole document; Insert Numbers is the number half of a column editor: without a column selection, it numbers the lines from the caret's line to the end at the caret's column.

**Deviations and findings:**
- **Undo of a large tool result blocks:** the 20 MB format is one GtkTextBuffer step, and undoing it blocked the main loop for 626.2 ms in the release build (618.7–644.3 ms in the debug build's CI runs). The tools themselves meet the bar.
- **Fixed during the work:** a trim that only deletes went in as one deletion and blocked the debug build's main loop for 339 ms, and the 20 MB format for 488.2 ms (now the coalescing runs on the worker and deletions go in 2 Mi characters at a time: 38.1 ms); Select to Matching Brace stopped before the far bracket; switching `smart_highlight` back on did not highlight again until the selection changed; Enter in a dialog's field did nothing; Rename File selected the whole name instead of the name without its extension (GtkText selects everything as it takes the focus).
- **Sandboxing the self-tests:** GLib refuses to trash on system mounts such as `/tmp`, and `xdg-mime` writes `~/.config/mimeapps.list`, so `context` and `default-editor` run with their own `HOME` and XDG directories (`sandbox-home`). `xdg-mime query default` reports a desktop entry only when its `Exec` program is on `PATH`; the stand-in entry uses `Exec=env %F`. After every run the user's `~/.config/mimeapps.list` still had its 2026-09-30 modification time and `~/.local/state/omarchy/defaults/editor` still held `code` (2026-09-18).
- **Omarchy's state path:** Stet writes the default editor to `$XDG_STATE_HOME/omarchy/defaults/editor` (as it reads the theme), and `omarchy-launch-editor` reads `$HOME/.local/state/omarchy/defaults/editor`; the two differ only if `XDG_STATE_HOME` is moved.
- **Comments** use the language's GtkSourceView metadata (`line-comment-start`, `block-comment-start`, `block-comment-end`); plain text and languages without them get a toast.
- **GtkSourceView's Ctrl+%** (the bracket jump) stays, next to Ctrl+B.
- **Preview gate:** [status bar and document map, tokyo-night](../tools/progress/public/m5-status-and-map-tokyo-night-2026-10-01.png), [Insert Numbers, catppuccin-latte](../tools/progress/public/m5-insert-numbers-catppuccin-latte-2026-10-01.png), [indentation popover, gruvbox](../tools/progress/public/m5-indentation-gruvbox-2026-10-01.png), [editor context menu, gruvbox](../tools/progress/public/m5-context-menu-gruvbox-2026-10-01.png), [Keyboard Shortcuts, tokyo-night](../tools/progress/public/m5-shortcuts-tokyo-night-2026-10-01.png); judged after the live checks below.

**Still to check in the live Hyprland session** (Norwegian layout, fcitx5 running):
1. The editor keys reach the text: Ctrl+D, Ctrl+L, Ctrl+Shift+L, Ctrl+Shift+X, Ctrl+T, Ctrl+Shift+Up and Down, Ctrl+J, Ctrl+Alt+Enter, Ctrl+Alt+Shift+Enter, Alt+Shift+U, Ctrl+U, Alt+U, Ctrl+Alt+U, Ctrl+Q, Ctrl+K, Ctrl+Shift+K, Ctrl+Shift+Q, Ctrl+B, Ctrl+Alt+B, Ctrl+Alt+F, Ctrl+Alt+Shift+F (and Ctrl+F3, Ctrl+Shift+F3 with Fn), Ctrl+Alt+Shift+M, C, J, B and X; Ctrl+C and Ctrl+X without a selection take the whole line.
2. In the editor, Alt+Left/Right, Alt+Shift+arrows, Ctrl+Shift+A, Ctrl+/ and Ctrl+\ do nothing, and Alt+Up/Down are still Find Previous and Next (M1) rather than GtkSourceView's line moves; with the focus in the find field, Ctrl+D, Ctrl+U and Ctrl+Shift+Up do not change the text.
3. Ctrl+Shift+U still opens fcitx5's Unicode entry, and Alt+Shift+U is not taken by anything else.
4. Right-clicking a tab opens its menu at the pointer, and the commands act on that tab; right-clicking the text shows the editor's menu with its submenus.
5. Open Terminal Here opens the Omarchy terminal in the file's folder, Open Containing Folder the file manager there; Move to Trash… on a scratch file puts it in the trash that Nautilus shows; Rename File… with Enter.
6. Settings › Open Keyboard Shortcuts: add `duplicate-line = ["<Alt>d"]`, save, and Alt+D duplicates at once; a mistake shows the toast with its line and column; the same edit saved from another editor applies too. Open Settings: `font_size = 14` applies at once.
7. The indentation popover by mouse and keyboard (the focus back in the text after Escape); the document map scrolls the text on a click and a drag.
8. Formatting a 20 MB JSON file (`write-generated`'s `json-lines` kind, or any large JSON array): the window keeps painting; then Undo's pause of about 0.6 s.
9. **Set as Default Editor…** changes the user's real default editor, which the standing authorization does not cover: it is the user's step. Afterwards SUPER+SHIFT+N opens Stet, and a double-click on a `.txt` file in Nautilus opens it in Stet (the M5 exit criterion).
10. `omarchy theme set` with the new UI open (the popover, the dialogs, the document map, the shortcuts dialog), light themes included; compare with the screenshots above.

**Follow-ups:**
- P2: undo and redo of a large tool result block the main loop (626 ms for a 20 MB format); splitting them needs a change to how ADR-003's undo works, if the user wants that bar for undo too.
- P2: M2's backup timer should take its interval from `Settings::backup_every()` (`backups`, `backup_interval`), and the M2 merge connects M2's settings to `Preferences`.
- P3: write the default editor to `$HOME/.local/state/omarchy` when `XDG_STATE_HOME` points elsewhere, matching `omarchy-launch-editor`.
- P3: Insert Numbers in a column selection after M6.

## M2 built and checked headless; live checks pending — 2026-10-01

Built in the M2 worktree from `a9349a4` (Wave A on `main`) under the standing authorization of 2026-10-01, on the merged session core (`infrastructure::session_store`, `domain::session`), while M5 was built in parallel in another worktree. All evidence is headless (GTK Broadway, private D-Bus buses); what needs the live session is at the end.

**What exists:**
- **Backups every 7 s** ([ADR-006 amendment](DECISIONS.md#adr-006-amendment--as-built-in-m2-2026-10-01)): the window builds the whole session and commits it with the text of every dirty document, and every untitled document with text, that changed since its last backup; a commit that would change nothing is skipped. The session holds the tabs in order, the active tab, the window's size, maximized state and zoom, the recent files, the find, replace, folder and filter histories, and per tab its path or untitled number, dirty flag, encoding, BOM, line ending, language, caret and selection (direction kept), first visible line, disk fingerprint, an encoding picked with Reinterpret, and whether saving is lossy. Over 16 Mi characters a document is backed up at most every 60 s and only after 2 s without edits; in large-file mode not periodically at all.
- **Quitting without prompts:** closing the window (SUPER+W), Quit (Ctrl+Alt+Q, Alt+F4) and logout hide the window, write and flush the last commit (at most 2 s, on a worker), answer `--wait` command lines and go. The only question left is Save / Discard / Cancel for documents in large-file mode with unsaved changes. Ctrl+W on a dirty tab still asks. Saved and closed documents' backups are removed by the next commit. Without a session (`--no-session`, or a store that couldn't open) quitting asks as before.
- **Signals:** SIGTERM, SIGHUP and SIGINT do the same without any prompt, large-file documents included (`glib-unix`).
- **Restore:** the store is opened on a worker, a lock retried for 2 s (still locked: no session in that window, with a toast). The window, zoom, recent files and histories come back; the active tab loads before the window is shown; every other tab is a stub (title, dirty dot, path) until it is shown or Save, Close All, Find All or Replace All in open documents, or Find in Files needs its text. A tab with unsaved changes comes from its backup, a clean one from its file through the file pipeline. A file changed since its backup gets M3's disk banner worded "The file “…” changed on disk since your unsaved changes." with Keep Mine and Load Disk Version (one undoable step), and stays a conflict across restarts until settled; a lost backup gets "The unsaved changes to “…” from the last session could not be restored." with Dismiss; orphans open as untitled tabs with a toast and are adopted by the next commit. Untitled numbers continue.
- **Forget Unsaved Drafts…** (File menu above Quit, and the palette; the registry has 68 actions): asks first, closes untitled tabs, reloads files with unsaved changes from disk, then deletes every backup and orphan through `forget_drafts()` on a worker.
- **D-Bus activation and `--wait`** ([ADR-011 amendment](DECISIONS.md#adr-011-amendment--d-bus-activation-and---wait-as-built-in-m2-2026-10-01)): `packaging/io.github.pixdevsapps.Stet.service` (installed by the PKGBUILD in `/usr/share/dbus-1/services/`), `DBusActivatable=true`, `HANDLES_COMMAND_LINE` and `HANDLES_OPEN`. When no Stet runs, a command line asks the bus to start it (`StartServiceByName`) and hands over its files as a remote. `--wait` holds the command line until its tabs close: status 1 when one closed unsaved or didn't load, or Stet quit while one had unsaved changes. `--wait` files stay out of the session and the recent files. Without a service file (development builds, `.Devel`) a `--wait` command line starts `stet --gapplication-service` itself. `--no-session` applies when it starts Stet. `--new-window` is not built (one window).
- **Recent files** live in `session.json`; M1's `recent.json` is migrated once, then deleted.
- **Model:** `WindowState::zoom`, `TabRecord::chosen_encoding`, `TabRecord::lossy`, `Session::directory_history` and `Session::filter_history`, all `serde(default)` and in the proptests; the backup policy `BackupState::action` and `TabRecord::anchor_and_caret`, unit-tested.
- **Self-tests:** `session`, `session-restore` and `activation` in CI, and `perf-session` and `preview-m2` by hand; `session-restore`, `activation` and `preview-m2` start child self-tests from `tests/selftest/session/` (eight, one and two). The harness runs child self-tests over a shared scratch directory (killed, signalled, quit, restored), `stet` processes in the background, and a private `dbus-daemon` whose only service directory holds Stet's own test service file.

**Checks** ([TESTING.md](TESTING.md#2026-10-01--m2-sessions-backups-restore-and---wait)):
- `STET_CI_DISPLAY=61 bash tools/ci.sh --ui` passed on the final code, `488664f`: fmt, Clippy `-D warnings`, 598 tests, the build, the smoke run and 1,801 self-test steps in 20 scripts, `session` 139, `session-restore` 24 (its children's steps reported inside) and `activation` 49 among them. It also passed on `d76c00a`; a run on `7b848b2` failed at Clippy (`type_complexity`), fixed.
- **SIGKILL with 43 dirty and untitled tabs** (20 edited files, 20 untitled, a CRLF file, a Windows-1252 file converted to UTF-8, an untitled tab set to Rust): the 7 s timer's backup commit came 6.64 s after the typing step began (on disk after 6.69 s; release build on `d76c00a`), and 6.62 s in the final CI run (debug build); after the kill every tab came back identical (text hash, dirty flag, selection with its direction, language, encoding, BOM, line ending), with the recent files and the find history (and, checked from `488664f` on, the window size and zoom), the active tab loaded and the others stubs; the typing after that backup was lost. Quitting by closing the window and SIGTERM kept the typing of their last moment.
- **50-tab restore:** the window's first frame, with the active tab's text, 157.5, 160.4 and 166.9 ms after the process started (release build, three runs; bar 500 ms); 168.2 ms in the debug build in the final CI run. Putting the 50 tabs in the strip took 44–51 ms of that.
- **Backup cost** (release build, scratch directory on btrfs as `~/.local/state` is, two runs): taking the text on the GTK thread 0.5 ms at 1 MB, 7.7–8.0 ms at 10 MB, 42.0–43.2 ms at 50 MB; the store's thread wrote them in 6.1–13.7, 7.5–17.3 and 19.9–25.1 ms. On tmpfs (the self-tests' default `/tmp`) the writes take under a millisecond, so those figures are not disk figures.
- **`--wait`:** with Stet running, saved and closed gave status 0 and Discard 1; with Stet not running, on a private bus, the service started by the bus and the development fallback both answered 0, and a self-test activated in the service's place gave 0, 1 and 1 (saved and closed, Discard, quit while dirty).

**Deviations and findings:**
- The restore conflict reuses M3's disk banner (and its old baseline) instead of a banner of its own; Compare comes with M7.
- A locked store after 2 s means another application id uses the same state directory (a development build next to the installed one): that window runs without a session rather than forwarding, which isn't possible across ids.
- The development fallback only spawns for `--wait`; a plain development `stet file` without a service file becomes the editor, as in M1, keeping its log in the terminal and keeping CI's smoke run as it was.
- `--wait` files are kept out of the recent files as well as the session (a commit message or a `sudoedit` copy is no recent file).
- Typing into a restored tab while its text loads is ignored: it is read-only while loading, as any loading file.
- Quit with no window (a service before its first window) now quits; before, Quit closed only windows, so an early Quit did nothing.
- **GTK exports window actions over D-Bus only on Wayland and X11**, not under Broadway, so the activation test drives the activated Stet through application actions and its session file, and runs a self-test as the service for the editing.
- **The license report had not run since M1:** it listed 87 crates; with M3's and M4's crates and `glib-unix` it lists 127, and THIRD_PARTY.md now says that `encoding_rs` adds BSD-3-Clause terms and `foldhash` is Zlib-licensed.
- Not kept in the session: undo history, the long-line path's origin, the mixed-line-ending warning, the NUL placeholder mapping, word wrap and whitespace (window settings, M5), pinned tabs (M8).
- Shared files touched for M5's merge: `app/src/window/mod.rs` (the session field, `add_page_with`, the close and selection hooks, `ForgetDrafts`), `app/src/editor.rs` (the tab's session state and last-change time), `app/src/application.rs` (rewritten around the session, activation's `open`, signals), `domain/src/actions.rs` (`ForgetDrafts`; `ALL` is 68), `app/src/actions.rs` (Quit), `app/src/selftest/*` (new commands and module), and the docs.

**Preview (headless):** [restored conflict, tokyo-night](../tools/progress/public/m2-restored-conflict-tokyo-night-2026-10-01.png), [lost changes, catppuccin-latte](../tools/progress/public/m2-lost-changes-catppuccin-latte-2026-10-01.png), [Forget Unsaved Drafts, gruvbox](../tools/progress/public/m2-forget-drafts-gruvbox-2026-10-01.png). Judged by the agent: the banners and the dialog are legible in a dark, a light and a warm theme, with the buttons in the theme's colours; the restored tabs show their dirty dots. The gate stands after the live checks below.

**Still to check in the live Hyprland session** (release build):
1. **SUPER+W, then relaunch:** with edited files, untitled tabs and fresh typing, close the window with SUPER+W: no prompt; `stet` again: every tab, the active one, the window size, the zoom and the typing are back.
2. **`systemctl --user stop`:** find Stet's unit (`systemctl --user status $(pgrep -xn stet)`; `dbus-:1.<n>-io.github.pixdevsapps.Stet@<n>.service` when the bus started it, an `app-…` unit from the launcher), type something, stop the unit within 7 s: the journal shows "SIGTERM: saving the session and quitting"; relaunch: the typing is back.
3. **SIGTERM and `kill -9`:** `kill -TERM $(pgrep -xn stet)` right after typing: back on relaunch. Type, wait 8 s, type more, `kill -9 $(pgrep -xn stet)`: on relaunch everything but the last typing.
4. **`GIT_EDITOR="stet --wait" git commit`** with a user-local service file: `~/.local/share/dbus-1/services/io.github.pixdevsapps.Stet.service` with `Exec=<absolute path to the release stet> --gapplication-service`, then `busctl --user call org.freedesktop.DBus /org/freedesktop/DBus org.freedesktop.DBus ReloadConfig` (dbus-broker). In a scratch repository, with Stet not running and then running: a tab opens; write, save and close it: the commit is made; again, close it with Discard: git aborts. With Stet not running, `systemctl --user list-units 'dbus-*Stet*'` shows Stet in a unit of its own, and closing the terminal leaves it running.
5. **`SUDO_EDITOR="stet --wait" sudoedit /etc/hosts`** (TESTING row 23): whether `sudo` passes the session bus to the editor; without it, `stet --wait` becomes the editor and returns when it quits.
6. **Reboot** with dirty and untitled tabs (TESTING row 3): everything is back after logging in again.
7. A launcher entry and a double-click in Nautilus with the package installed: Stet starts through D-Bus (`DBusActivatable=true`) and opens the file.
8. A 50-tab session's start on Hyprland feels instant (bar 500 ms), and the conflict and lost-changes banners and Forget Unsaved Drafts in the live themes.

**Follow-ups:**
- P0 (M2 exit): the live checks above.
- P1: the backup interval and backups on or off (the plan's M2 settings) come with M5's `config.toml`; `BACKUP_INTERVAL` is a constant until then.
- P2: keep the long-line origin, the mixed-line-ending warning and the NUL placeholder mapping in the session.
- P2: update AGENTS.md's "Current scope" and README's status paragraph ("Unsaved work is not yet restored after quitting") when M2 and M5 are merged; left alone here to keep the merge simple.
- P3: a 50 MB backup takes 42 ms on the GTK thread, two copies of the text (GTK's and the store's `Arc<str>`); a store API that takes the text GTK returned could save one.
- P3: SIGHUP and SIGINT share SIGTERM's handler, but only SIGTERM is tested.
- P3: `--new-window`, if a second window is ever wanted.

## Live acceptance on Hyprland; Wave B and M6 under way — 2026-10-01

**Live acceptance** (`7387fb0`, [TESTING.md](TESTING.md#2026-10-01--live-acceptance-script-on-hyprland-wave-a-build)): `tools/live/acceptance.py` ran the Wave A release build in the user's Hyprland session under the standing authorization, and all 18 default checks passed: startup median 284.2 ms; a second launch focuses the window and opens `FILE:40`; theme switch on screen in 201 ms and `omarchy-theme-refresh`; a live font change; drag-and-drop of a file list; typing lands in the editor; SUPER+C/V/X; the F-key-free shortcuts and the Norwegian `+` key; floating portal choosers; the unsaved prompts for Ctrl+W and SUPER+W; background reloads that never take focus; the dirty-file and read-only banners; three legacy encodings; the 94 MiB fixture's first screen 987 ms from launch; typing right after opening a 10 MB file; the find bar's replace row, Find All and result navigation. This answers item 1 of the Wave A list above and parts of the manual acceptance rows 1, 2, 7, 10, 11, 14–19 and 21 (marked in the table). The script itself is described in [BUILD.md](BUILD.md#live-acceptance-script). Its own fixes: AT-SPI lookups now tolerate accessibles that GTK replaces while a dialog opens, and key latency is timed from the end of each injection (a `send_key_state` key press costs about 200 ms).

**Under way:** Wave B, M2 (never lose work) and M5 (text toolbox and chrome), each in its own worktree from `a9349a4`; and M6 (column mode), started ahead of the sequence in a third worktree from `7387fb0`, because it is the riskiest 1.0 item and touches little that M2 and M5 change. M8 waits for M2 (tabs and the session), and M7 for both. Each agent keeps headless runs to its own display range (M2 60–69, M5 70–79, M6 80–89).

## Wave A merged: M3 and M4 — 2026-10-01

Merged in the Wave A worktree from `c3d5e4b` (`main`: M1 and the live acceptance driver `tools/live/acceptance.py`) under the standing authorization of 2026-10-01: M3 (`ce58808`) first, then M4 (`d4e1c40`), each with `git merge --no-ff`, then two fix commits. All evidence is headless (GTK Broadway); what needs the live session is at the end.

**Merge** (`ede62fa`, `a115419`):
- M3 merged without conflicts. M4 conflicted in `app/src/window/mod.rs`, `app/src/editor.rs`, `app/src/main.rs`, `domain/src/actions.rs`, `app/src/selftest/script.rs`, `tools/ci.sh` and four docs, resolved so that both milestones work:
  - **Layout:** M3's banners stay inside each tab, above its editor; M4's find bar sits right below the tab view, with the results panel under both in a vertical paned; the status bar has M3's line-ending and encoding buttons ([DESIGN.md](DESIGN.md#as-merged-in-wave-a--2026-10-01)). The window module holds M3's files (`banners`, `disk`, `encoding`, `encoding_picker`, `files`) and M4's (`fif`, `replace`, `results`, `smart`).
  - **Editor page:** M3's document model and banners with M4's per-tab `PageSearch` and its match tag.
  - **Registry:** 67 actions (M1 36, M3 16, M4 15) with unique names and keys, checked by the registry's unit tests; the menu is File, Edit, Search, View, Encoding, Language, Help.
  - **Self-test harness:** both milestones' commands. M4's `load-text` used M1's `set_loaded_text`, which M3 removed, so `perf-replace.stet-test` opens its 149 MB fixture through the file pipeline (large-file mode) and `load-text` is gone; M4's replace code follows M3's `read_only_message`, which returns `&str`.
  - **CI:** `tools/ci.sh` skips exactly `preview*` and `perf-*`. M3's `typing-control.stet-test`, which is meant to fail, is now `perf-typing-control.stet-test`.
  - **Docs:** both dated entries (M3's first, as it finished last), both evidence entries, both design entries, both authorization rows, and the ADR index with both milestones' amendments.

**Integration fixes** (`f9b1a58`, [ADR-005 amendment](DECISIONS.md#adr-005-amendment--search-over-m3s-documents-wave-a-2026-10-01)):
- Highlight-all, our matcher's highlight tag and smart highlighting follow M3's `EditorPage::large_file_mode()`; M4's stand-in `page_search::is_large` is gone. The find bar applies the mode as soon as a loading file's size is known: before that, a 50 MiB log opened with the bar open kept highlight-all on (the new test fails without the change).
- `search-crlf.stet-test` runs all its steps now: `require-lf-buffer` and the TAP skip are gone. Two of M4's expected counts assumed the caret at the start (after Undo the count was "2 of 2", not "1 of 2"), so the script puts it there; it also saves a `\r\n` replacement and checks the CRLF bytes.
- New `search-decoded.stet-test`: Windows-1252 and Shift_JIS with CRLF (find, regex, Extended `\r\n`, a Replace All saved back in the encoding), Extended `\0` and a typed ␀ against NUL in UTF-16 and in a binary-looking file, Find in Files over an open Windows-1252 file, and large-file mode with the find bar open.
- Find in Files: the path for documents with unsaved changes already searched the buffer's decoded text whatever the encoding (the new test checks it with a Windows-1252 file). But a clean Windows-1252 tab was searched from its file, as bytes, so "garçon" was not found in it; now every open document whose file the walk would read differently (an encoding other than UTF-8 or UTF-16 with a BOM, decoding errors, NUL, CR line endings) is searched from its decoded text too.

**The focus bug from the live session** (`d589d8e`): `stet file.txt` left the keyboard focus on the status bar's language button. Opening the file switched tabs before the window was first shown, and hiding the untitled tab's editor made GTK queue a move of its focus, which GTK runs after the next frame and cancels only when the focus is set again inside a visible window; after the first frame it moved the focus to the next focusable widget after the editor, the language button. Closing a popover did the same: the palette left the focus on the paned, the pickers and menus on their buttons. Now the window takes the focus again when it is mapped, and gives it back to the current editor after a file opens or finishes loading, after a tab switch, and when the palette, a status-bar popover, the hamburger menu, a dialog or a banner button is done, unless the user is in the find bar, the results panel, an open popover or a dialog ([DESIGN.md](DESIGN.md#as-merged-in-wave-a--2026-10-01)). Ctrl+Tab from the find field keeps the focus there; a click on a tab gives it to the editor. `focus.stet-test`, in CI, covers each flow; it starts the app with a file and shows the window only once the file is open (the live order), which reproduced the live failure headless before the fix.

**Checks** ([TESTING.md](TESTING.md#2026-10-01--wave-a-m3-and-m4-merged-integration-and-focus-fixes)): `STET_CI_DISPLAY=51 bash tools/ci.sh --ui` passed on the final code, `d589d8e`: fmt, clippy `-D warnings`, 591 tests, the build, the smoke run and 1,589 self-test steps in 17 scripts, `focus` 142, `search-decoded` 110 and `search-crlf` 33 among them. On the merge commits alone it had failed in `search-crlf` (27 of 29), which the fixes above settled. M3's large-file timings in the debug build are as before: 100 MB first screen 1,519.2 ms, longest main-loop block 45.5 ms; 200 MB all in after 5,993.2 ms, longest block 55.6 ms.

**Still to check in the live Hyprland session:**
1. `tools/live/acceptance.py` with the release build: the check `typing_focus` (`stet focus.txt`, then type), and the rest of the acceptance run.
2. The focus with real input ([manual acceptance](TESTING.md#manual-omarchy-acceptance), row 21): the Open dialog through the portal, a recent file, Ctrl+Tab and a click on a tab, Escape out of the find bar, the palette, the pickers and the menus, the save prompt answered from the keyboard, a banner button clicked; Ctrl+Tab from the find field keeps typing in the field.
3. M3's live list (12 items) and M4's (items 1–8; its item 9, `search-crlf` without skips, is done headless here), in their entries below.
4. The live timings of M4's Replace All and Find in Files. Headless, `perf-replace.stet-test` (not in CI) still passes with the release build now that it opens the 149 MB fixture through the file pipeline: 1,000,000 replacements as one bulk edit in 2,277.3 ms end to end, the GTK thread blocked 1,916.8 ms while applying, and one undo step (1,493.6 ms) that restores the text exactly; M4 measured 2.30–2.34 s.

**Follow-ups:**
- Done: M3's P1 "search should respect `large_file_mode()`" and M4's P2 "replace `page_search::is_large`".
- P1 (M2): `--wait` for the `sudoedit` command, the baselines and the search histories in `session.json` (from M3's and M4's lists).
- P2: the plan tags M4 as 0.2; no tag was made here (tags were not part of this work).
- P3: the find bar's own popovers (the history drop-downs, ⋮) leave the focus on their button when closed with Escape, GTK's default; the editor rules above don't apply inside the find bar.
- P3: a click on the tab that is already selected puts the focus on the tab itself (libadwaita's keyboard navigation of tabs), so typing goes nowhere until the editor is clicked.

## M3 built and checked headless; live checks pending — 2026-10-01

Built in the M3 worktree from `bc498b3` (M1 merged) under the standing authorization of 2026-10-01, while M4's search UI was built in parallel in another worktree. M2's session, D-Bus activation and `--wait` are not part of this work. The encoding, safe-save and session cores that existed before M3 are used as they were, except where an ADR amendment below says otherwise. All evidence is headless (GTK Broadway); what needs the live session is listed at the end.

**What exists:**

- **One file pipeline** ([ADR-007 amendment](DECISIONS.md#adr-007-amendment--as-built-in-m3-2026-10-01)): every open and save goes through `infrastructure::fs` on a worker. M1's UTF-8-only `text_file` and its refusals (not UTF-8, NUL, lines over 64 KiB, files over 50 MB) are gone. Each tab has a document model (`editor::DocState`: encoding and BOM, line ending and its counts, detection, decode flags, read-only reason, baseline fingerprint, disk state, where the text came from) behind the status bar, the banners and the next save.
- **Status bar:** the line ending (`LF`, `CRLF`, `CR`, `Mixed`) opens the line-ending menu; the encoding (`UTF-8`, `UTF-8 BOM`, `UTF-16 LE BOM`, `Windows-1252`, `Shift_JIS`, …) opens the **encoding picker**, a popover with Reinterpret As / Convert To, a filter, and every encoding by group.
- **Encoding menu** between View and Language, in a familiar order: Encode in UTF-8, UTF-8-BOM, UTF-16 BE BOM, UTF-16 LE BOM; Character Sets › one submenu per group; Convert to ANSI (Windows-1252), UTF-8, UTF-8-BOM, UTF-16 BE BOM, UTF-16 LE BOM. Also File › Reload from Disk and Edit › EOL Conversion › Windows (CR LF), Unix (LF), Macintosh (CR). The registry has 52 actions (M1: 36). The new ones have no keys and are in the palette, where "Encoding…" opens the picker.
- **Reinterpret** reads the bytes again in another encoding, asking first when that discards unsaved changes. **Convert** checks on a worker that every character exists in the target encoding; otherwise the **unmappable dialog** lists the first 20 missing characters with line, column and code point, each with Go To, gives the total, and offers Convert to UTF-8 (Save as UTF-8 when a save met them) or Cancel.
- **Loading** ([ADR-003 amendment](DECISIONS.md#adr-003-amendment--loading-and-reloading-files-2026-10-01)): read and decode on a worker; ~4 MiB pieces streamed from a loader thread and inserted 16 ms apart into a read-only view, with a progress banner and a spinner on the tab; then the language and the highlighting policy, undo, the unmodified state, the baseline fingerprint, the file monitor and the caret. From 50 MiB, **large-file mode**: no highlighting, word wrap, whitespace or bracket matching, and a banner; `EditorPage::large_file_mode()` is there for M4. From 256 MiB a file is refused, with the reason, before anything is read.
- **Highlighting limits** ([ADR-014 amendment](DECISIONS.md#adr-014-amendment--the-limits-are-2-mib-and-100000-lines-2026-10-01)): 2 MiB and 100,000 lines, no delayed start. Above them the buffer gets no language; the status bar still shows the detected one, the picker still sets it, and a banner offers Highlight Anyway.
- **Long lines** (over 50,000 characters): a dialog offers Open Formatted (JSON or XML through `domain::ops`, on a worker; saving the formatted text asks first) or Open Read-Only (lines broken every 1,000 characters for display; saving is off). There is no "Open anyway". Text that doesn't parse opens read-only with display breaks, and a toast says why.
- **Banners** ([DESIGN.md](DESIGN.md#as-built-in-m3--2026-10-01)) belong to the tab, stack, and each has its own buttons: loading, changed on disk (Reload, Keep Mine), deleted (Save), read-only (Copy Command for `sudoedit`, Edit Anyway), binary (Edit Anyway when it round-trips), lossy decode (Reinterpret…), placeholder conflict, mixed line endings (Normalize, Keep), long lines, large file, highlighting off (Highlight Anyway).
- **Saving** on a worker from a snapshot through `save_document`: a temporary file, fsync and rename, keeping mode, owner and symbolic links, or an in-place write after a backup copy when a rename can't keep them. The result is the new baseline. Saving asks first after a lossy decode, for formatted long-line text, and with mixed line endings. A save removes `.<name>.XXXXXX.stet-tmp` files older than a minute that an interrupted save left next to the target. Unmappable characters open the dialog; permission denied and read-only file systems show a banner (the `sudoedit` command, or Save As); a save interrupted partway shows a dialog naming the backup of the previous content.
- **Outside changes** (`app/src/window/disk.rs`): a file monitor per open file (renames into place included, 300 ms debounce), and a check of every open file when the window gets focus (at most once a second) for file systems that send no events. A change is a fingerprint that no longer `matches` the baseline. A clean document reloads quietly as one undoable step, keeping the caret and scroll position, with a toast. With unsaved changes a banner offers Reload (Undo brings the edits back) or Keep Mine. A deleted file gets a banner and counts as modified; a clean file deleted and written again (a checkout) reloads. Nothing switches tabs or takes focus.
- **One fingerprint type** ([ADR-006 amendment](DECISIONS.md#adr-006-amendment--one-fingerprint-type-2026-10-01)): `stet_domain::session::Fingerprint`, compared with `matches`, which ignores the device number.
- **The ADR-014 measurement** in `s1_perf` (`threshold`, `threshold-more`, `threshold-lines`, `threshold-untagged`; [S1 addendum](spikes/S1-performance.md#addendum-the-adr-014-follow-up-m3-2026-10-01)).
- **Self-tests** `encodings` (every fixture), `convert`, `disk` and `large`, and M1's `files` updated; `typing-control` (the stall with highlighting forced on, kept as a control) and `preview-m3` (screenshots) are not in CI. The large-file timings use a Broadway web client that the self-test connects (`app/src/selftest/broadway.rs`), so frames are not held to Broadway's ~1 fps.

**Checks** ([TESTING.md](TESTING.md#2026-10-01--m3-build-ci-self-tests-large-files-and-the-adr-014-measurement)): `STET_CI_DISPLAY=31 bash tools/ci.sh --ui` passed on the final code, `241b0d4` (and before on `8ec7e5c` and `873a053`): fmt, clippy `-D warnings`, 577 tests, the build, the smoke run and 893 self-test steps in ten scripts. Headless, release build:
- 100 MB log: first screen 164.7 ms after the open (192.7 ms in an earlier run; bar 1.5 s), all of it in after 1.45 s, longest main-loop block 46.6 ms.
- 200 MB log: large-file mode, first screen 237.1 ms, all in after 3.0 s, longest block 56.0 ms; typing works.
- 10 MB single-line JSON, Open Formatted: 20.3 MB of text, first screen 139.3 ms, all in after 468 ms, longest block 82.4 ms.
- Typing 30 keys into a 10 MB Rust file right after it opened: 3.4 ms median, 11.7 ms at worst; with Highlight Anyway 41.9 ms median, 325.3 ms at worst.
- Memory: the process's RSS grew by 314,468 kB while the 100 MB file opened, 3.07× the file size; S1's bar is 3×. Measured right after the load, before the allocator settled.

**Preview (headless):** [stacked banners, tokyo-night](../tools/progress/public/m3-banners-tokyo-night-2026-10-01.png), [encoding picker, catppuccin-latte](../tools/progress/public/m3-encoding-picker-catppuccin-latte-2026-10-01.png), [unmappable dialog, gruvbox](../tools/progress/public/m3-unmappable-gruvbox-2026-10-01.png), [changed on disk, tokyo-night](../tools/progress/public/m3-changed-on-disk-tokyo-night-2026-10-01.png), [long-line dialog, tokyo-night](../tools/progress/public/m3-long-lines-tokyo-night-2026-10-01.png), [formatted JSON, tokyo-night](../tools/progress/public/m3-formatted-tokyo-night-2026-10-01.png). The gate is judged after the live checks below.

**Deviations and findings:**
- **Line-ending conversion is not an undoable step** ([ADR-008 amendment](DECISIONS.md#adr-008-amendment--as-built-in-m3-and-line-endings-outside-the-undo-history-2026-10-01)); converting back is the undo. Reloading a large file or long-line text, and Reinterpret, load again from scratch without undo; every other reload is one undo step.
- **"Highlighting off" means no language on the buffer:** GtkSourceView's context engine analyses the whole buffer even with `highlight-syntax` off, at a priority above redraws. M5's comment toggling must use `EditorPage::language()`, not the buffer's language.
- **The status-bar encoding control is a popover of its own**, not a menu: GtkPopoverMenu names submenu pages by their labels, so the same groups under Reinterpret and Convert collided (17 GTK warnings, the second set unreachable).
- **A Latin-1 file opens as Windows-1252, editable**; M1 opened it read-only. `files.stet-test` follows.
- **The debug build** shows the first screen of 100 MB after about 1.5 s (1,491.8 ms and 1,522.6 ms in the last two CI runs) because decoding takes 1.4 s unoptimized; the self-test holds release builds to 1.5 s and debug builds to 3 s, so CI, which runs the debug build, doesn't enforce the 1.5 s bar.
- **The read-only banner's command** is `SUDO_EDITOR="stet --wait" sudoedit <path>`. `--wait` comes with M2; until then the copied command stops at clap's "unexpected argument '--wait'".
- Nothing writes `session.json` yet; the baselines are the values M2 will store.

**Still to check in the live Hyprland session** (release build; `python3 tools/gen-fixtures.py` writes the inputs to `target/fixtures/`):
1. Open `target/fixtures/big-1m-lines.txt` (98 MB): the first screen within 1.5 s of the open, the progress banner and tab spinner while it loads, the window responsive (scroll, switch tabs, the menu), then the large-file banner.
2. A 200 MB log (`yes "2026-10-01 12:00:00 INFO a log line for the large-file check" | head -c 209715200 > <scratch>/200mb.log`): opens in large-file mode without freezing; typing and Undo work; the RSS (`grep VmRSS /proc/$(pgrep -n stet)/status`) a few seconds after it loaded.
3. Type at once after opening `target/fixtures/medium-10mb.rs`: no lag, and the "Syntax highlighting is off" banner; after Highlight Anyway the stall of ADR-014 is expected. S1 measured 70–77 ms median live right after load with highlighting.
4. `target/fixtures/single-line-10mb.json`: the long-line dialog; Open Formatted shows formatted JSON in about half a second without freezing, and Save asks first; Open Read-Only refuses to save with a toast.
5. Outside changes from real tools, with the file's tab in front and in the background: `git checkout`/`git stash` of a clean open file, `sed -i`, and a save from another editor reload quietly with a toast, keeping the caret and scroll; with unsaved edits the Reload / Keep Mine banner; `rm` gives the deleted banner and a `*` in the title. Stet must not switch tabs, take focus or raise itself while the terminal has focus.
6. If a network or FUSE mount (sshfs, NFS) is at hand: change an open file on the server, then focus Stet: it reloads.
7. A file you may not write (`/etc/hostname`): the read-only banner with the `sudoedit` command; Copy Command puts it on the clipboard (paste it into a terminal); Edit Anyway and Save give the "You may not write" banner; Save As works.
8. Real files from other tools: `printf 'caf\xe9\r\n' > cp1252.txt`, and `iconv -f UTF-8 -t SHIFT_JIS`, `-t GB18030` and `-t UTF-16` of a UTF-8 text: the status bar shows `Windows-1252` and `CRLF`, `Shift_JIS`, `GB18030`, `UTF-16 LE BOM`; the text is right; an edit and Save keep the encoding (`od -An -tx1`; `xxd` isn't installed).
9. The Encoding menu, Character Sets, the status-bar picker (filter, both modes, keyboard only too) and the line-ending menu; type `漢字` into a Windows-1252 file and save: the unmappable dialog, where Go To selects the character.
10. A file with mixed line endings: the banner, Normalize, then Save writes one kind (`file` or `od -c`).
11. A symbolic link to a file: open the link, edit, save: `readlink` still points to the target, which has the new text.
12. The new UI in tokyo-night, catppuccin-latte and gruvbox: stacked banners, the picker, the unmappable and long-line dialogs, and the status-bar buttons are legible; compare with the screenshots above.

**Follow-ups:**
- P0 (M3 exit): the live checks above, and the preview gate.
- P1: when merging with M2 and M4: `--wait` for the `sudoedit` command and the baselines in `session.json` (M2); search should respect `large_file_mode()` (M4). `app/src/window/find.rs` and `app/src/search.rs` were not changed here.
- P2: a settled RSS measurement for the 100 MB open, against S1's 3× bar.
- P2: the interrupted-save dialog has no automated test; it needs a write that fails partway.

## M4 Search built and checked headless; live checks pending — 2026-10-01

Built in the M4 worktree under the standing authorization of 2026-10-01, on the merged search core (`domain::search`, `infrastructure::search`, `infrastructure::find_in_files`) and M1's shell, while M3 was built in parallel; this branch still has M1's UTF-8 loader, whose buffer keeps CRLF. All evidence is headless (GTK Broadway).

**What exists:**
- **Find bar** ([DESIGN.md](DESIGN.md#as-built-in-m4--2026-10-01)): Find, Replace and Find in Files as rows of the bar below the editor (Ctrl+F, Ctrl+H, Ctrl+Shift+F); Normal, Extended and Regex modes, match case, whole word (ignored in Regex mode), Wrap around, In selection, Backward direction, `.` matches newline; Find Next and Previous, Count, Replace, Replace All, Find All in Current Document, Find All in Open Documents, Replace All in Open Documents; Enter and Shift+Enter; "n of m"; pattern errors at the typed character (error style, tooltip, wavy underline, message) without taking the focus; the replace field explains templates that are easy to get wrong (`Template::warnings`).
- **Histories** for the find, replace, folder and filter fields (10 entries each; Up and Down, and a drop-down for find and replace), in memory, with an API for M2: `Window::search_history()` and `set_search_history()` over `domain::search::SearchHistory`, whose `find` and `replace` map to `session.json`'s `find_history` and `replace_history` (most recent first).
- **Highlighting and navigation** ([ADR-005 amendment](DECISIONS.md#adr-005-amendment--search-as-built-in-m4-2026-10-01)): one translation and our matcher for every query; GtkSourceView's `SearchContext` counts, highlights and navigates what it can (regex always on, case from the translation, `at-word-boundaries` off); our matcher does `\K`, `\G` and empty matches on a worker, with a highlight tag in the scheme's `search-match` colours. Each tab has its own search settings. Large-file mode (50 MiB, a simple size check until M3's document flag) turns highlight-all and smart highlighting off.
- **Smart highlighting** of a selected whole word on one line (match case, whole word), through a second search context in `stet:smart-highlight`, debounced (120 ms).
- **Replacing** only through our engine on snapshots on workers, applied on the GTK thread in one user action per document, as one bulk edit above 2,000 edits; Replace on a selection that is not a match only finds the next one; Replace All in Open Documents asks first. `SearchContext::replace`/`replace_all` stay unused.
- **Results panel** below the find bar (resizable, closable, at most 10 searches): searches, files and hit lines with the matches emphasized; double-click or Enter jumps (opening the file if needed); Next and Previous Search Result step through every match; Search Results Window moves the focus in and out; Copy, Clear, Close.
- **Find in Files**: folder (the current file's, else the last used, else home), filters (`*.*`), Subfolders, Hidden, `.gitignore`, the bar's query; results stream into the panel every 33 ms; Stop; a summary (hits, files, files searched, time, stopped or capped, unreadable files). Open documents with unsaved changes are searched from their text (`FifRequest::open_documents`, a core extension).
- **Registry:** 15 new actions (51 in all) with the keys in the [ADR-009 amendment](DECISIONS.md#adr-009-amendment--search-keys-m4-2026-10-01): Ctrl+H, Ctrl+Shift+F, and Ctrl+Alt+R, Ctrl+Alt+Down and Ctrl+Alt+Up next to F7, F4 and Shift+F4.
- **Self-tests** `search`, `results`, `fif`, `parity`, `search-crlf` in CI; `preview-search`, `perf-replace` and `perf-find-in-files` by hand ([TESTING.md](TESTING.md#ui-self-tests-stet---self-test-from-m1)).

**Checks** ([TESTING.md](TESTING.md#2026-10-01--m4-search-ci-self-tests-widget-parity-and-performance)): `STET_CI_DISPLAY=41 bash tools/ci.sh --ui` passed: fmt, clippy `-D warnings`, 578 tests, the build, the smoke run and 786 self-test steps in 11 scripts (25 CRLF steps skipped, see below).
- **Widget parity:** 110 of the parity table's 122 rows ran through a real `GtkSourceSearchContext` with spans and counts identical to our matcher's; 3 invalid patterns failed in both; 8 rows are routed to our matcher; a deliberately broken control run failed, as it should.
- **Replace All over 1M lines** (S2's 149 MB fixture, release build, four runs, the last on the final build): 2.30–2.34 s end to end (bar: 5 s), one undo step; the GTK thread was blocked 1.94–1.98 s by applying the bulk edit (about 0.39 s deleting and 1.55 s inserting), then, in the three runs that measured it, never more than 58 ms during the background rescan; undo took 1.53–1.56 s and restored the text exactly. Against the proposals awaiting review ("blocked ≤ 2 s", "undo ≤ 2 s with RSS growth ≤ 2× the file size"): met, the first narrowly; undo grew RSS by at most 16 MiB.
- **Find in Files over `~/Projects`** (warm page cache, final build): first file found after 0.7–23 ms and first results on screen after 34–66 ms (bar: 300 ms); 47,879–47,918 files in 70–127 ms with the defaults, 472,284 with hidden and ignored files in 0.47 s, and the 50,000-hit cap reached in 1.27 s; the main loop never stalled for more than 48.1 ms. Stop ended an ordinary search in 15 ms; a backtracking-heavy regex needed 0.42 s, because one PCRE2 scan over a long buffer cannot be interrupted (a documented limit of the core).

**Deviations and findings:**
- **Find in Files searched hidden and ignored files** whenever an include filter was set, the default `*.*` included: `ignore`'s whitelist overrides win over its hidden and `.gitignore` rules. Fixed in the core: only exclusions are walk overrides; inclusions are checked per file (new unit test).
- **Find in Files results are listed in the order the parallel walk reports them,** not sorted: inserting them one by one in sorted order blocked the main loop for 433 ms per drain with 13,000 matching files. Files still queued when Stop is pressed are dropped.
- **The find bar moved** from the toolbar's bottom bars into the content, so it stays right below the editor with the results panel under it, as in the DESIGN mock.
- **In selection** also remembers a multi-line selection the bar opened with, because the incremental search moves the caret; it affects only Count and Replace All.
- **Smart highlighting matches case by default,** as the M4 brief asked.
- **Replace All in Open Documents asks for confirmation** (a decision dialog); each document gets its own undo step.
- **Extended `\r\n` in a CRLF document:** this branch's buffer keeps CRLF (M1), so `search-crlf.stet-test` skips its 25 checks with TAP `# SKIP` until M3's LF normalization is merged; the engine-level row passes.
- **Ctrl+Alt+R** is the Search Results Window; Stet has no use for Text Direction RTL, the key's familiar meaning ([ADR-009 amendment](DECISIONS.md#adr-009-amendment--search-keys-m4-2026-10-01)).
- **Preview gate:** [replace, tokyo-night](../tools/progress/public/m4-replace-tokyo-night-2026-10-01.png), [Find in Files, catppuccin-latte](../tools/progress/public/m4-find-in-files-catppuccin-latte-2026-10-01.png), [pattern error and smart highlighting, gruvbox](../tools/progress/public/m4-error-and-smart-highlight-gruvbox-2026-10-01.png); judged after the live checks below.

**Still to check in the live Hyprland session:**
1. Keys reach the app: Ctrl+H, Ctrl+Shift+F, Ctrl+Alt+R (and F7 with Fn) into and out of the results panel, Ctrl+Alt+Down/Up (and F4/Shift+F4 with Fn); in the find bar Enter, Shift+Enter, Escape, Up and Down; in the panel Enter, Escape and Ctrl+C.
2. Double-click on a result line jumps to it; dragging the panel's divider resizes it and the height is kept after closing and reopening.
3. The history drop-downs open, list entries and fill the field on click and with Enter.
4. The folder chooser (the folder button in the files row) floats and fills the folder field.
5. `omarchy theme set` with the bar and the panel open: the bar, the panel, the match emphasis and smart highlighting recolour live, light themes included.
6. Find in Files over `~/Projects` and Replace All over the 1M-line fixture in the live session with the release build: the timings, and that the window keeps painting while Find in Files streams.
7. Smart highlighting after a double-click on a word.
8. At a narrow width (a quarter of the screen) the find bar's rows still fit or shrink sensibly.
9. After the M3 merge: `search-crlf.stet-test` runs its 25 steps (no `# SKIP`).

**Follow-ups:**
- P2: persist the histories in `session.json` (M2), and the folder and filter histories too (new session fields).
- P2: replace `page_search::is_large` with M3's large-file flag after the merge.
- P3: a bulk edit blocks the GTK thread for about 1.9 s at 1M lines; inserting the result in chunks inside the one user action (the view read-only meanwhile) could cut the stall, if the user wants the stricter bar.
- P3: Find in Files timings with a cold page cache (needs root to drop caches).
- P3: Stop can take up to about 0.4 s with a backtracking-heavy regex over files with long lines (one PCRE2 scan is not interruptible); smaller search buffers or a match limit for Find in Files could bound it.
- P3: sort Find in Files results once the search ends, if the order matters to users (sorting while streaming was too slow).

## M1 built and checked headless; live checks pending — 2026-10-01

Built in the M1 worktree under the standing authorization of 2026-10-01, UTF-8 only; the toolkit-free cores of later milestones (session store, encodings and safe save, search engine, text operations, column math, compare, marks, fuzzy matching) are being built separately and are not part of this work. All evidence below is headless (GTK Broadway); what needs the live session is listed at the end.

**What exists:**

- **Action registry** (`domain/src/actions.rs`): 36 actions, each with a stable GAction name, scope, label, menu place (File, Edit, Search, View, Language, Help), keys and palette visibility. The hamburger menu, the command palette and the accelerators are generated from it; editing keys (undo, clipboard, select all) stay GtkTextView's own bindings and are only shown. Unit tests: unique names, no key used twice, well-formed accelerators, no action reachable only through an F-key, US and Norwegian punctuation rules, fcitx5 and Hyprland grabs avoided. The self-tests also run every accelerator through GTK's own parser, check that every shown editing key is a real GtkTextView binding, and check that the menu holds exactly the registry's actions.
- **Window** ([DESIGN.md](DESIGN.md#as-built-in-m1--2026-10-01)): AdwTabBar with the hamburger menu, no toolbar and no title bar (window controls only outside Hyprland), the editor, the find bar and the status bar (Ln/Col/Sel, language picker, line ending, `UTF-8`, INS/OVR); square chrome.
- **Tabs and files, UTF-8 only:** new (`Untitled N`), open (file chooser, several files), save and Save As on a worker thread (a temporary file in the same directory, fsync, rename, the file mode kept, a symlink followed), Save All, close with Save / Discard / Cancel, Close All, Restore Closed Tab (untitled text included), reordering, the dirty dot and `*` in the window title `file — dir — Stet`. Files that are not UTF-8 or contain NUL open read-only with a banner and are never rewritten; files over 50 MB or with lines over 64 KiB are refused with a toast until M3's large-file and long-line work.
- **Recent files** in `$XDG_STATE_HOME/stet/recent.json` (directory 0700, file 0600), in File › Open Recent and the palette. **Drag and drop** of files anywhere on the window opens them (a capture-phase drop target, so the text view never gets the URI).
- **Syntax detection:** file-name, shebang, declaration and modeline hints in `domain/src/language.rs`, then GtkSourceView's globs and content types; the language picker in the status bar and Language › Set Language….
- **Find** (Ctrl+F): incremental search, match case, match count, the current match in the theme's `stet:search-current` style, Enter and Shift+Enter, Find Next and Previous, Escape. **Go to Line** (Ctrl+G; `line` or `line:col`), **word wrap**, **show whitespace**, **zoom** (keys and Ctrl+scroll), **full screen**.
- **Command palette** (F1, Ctrl+Shift+P): registry actions with their menu path and keys, the recent files, and `:line[:col]`.
- **Single instance and the command line** ([ADR-011 note](DECISIONS.md#adr-011-note--single-instance-in-m1-2026-10-01)): `stet a.rs b.txt:40` in a second terminal opens both files in the running window at the right line; `file:line:col`, `-n`, `-c`; clap parses locally and again in the primary, which never exits on bad input.
- **Live Omarchy theme and font** ([ADR-004 amendment](DECISIONS.md#adr-004-amendment--as-built-in-m1-2026-10-01)), including `current/` appearing after start.
- **Exact tab stops** ([ADR-015 amendment](DECISIONS.md#adr-015-amendment--re-applied-after-gtksourceviews-own-resets-2026-10-01)) and the **undo cap of 1,000 steps** ([ADR-003 amendment](DECISIONS.md#adr-003-amendment--the-undo-cap-is-1000-steps-2026-10-01)).
- **`stet --self-test <script>`** with six CI scripts and a preview script ([TESTING.md](TESTING.md#ui-self-tests-stet---self-test-from-m1)); `tools/ci.sh --ui` runs them.
- **Packaging** (Hertz pattern): `packaging/PKGBUILD.in`, `tools/package-source.py` (tarball of the committed tree), `tools/license-report.py` (87 crates, texts in `packaging/licenses/`), the desktop file, an original icon ([ASSETS.md](ASSETS.md)), `THIRD_PARTY.md`.

**Keys chosen for the F-row** ([ADR-009 amendment](DECISIONS.md#adr-009-amendment--f-key-free-keys-and-the-norwegian-layout-2026-10-01)): Ctrl+Shift+P for the palette (F1), Alt+Down and Alt+Up for Find Next and Previous (F3, Shift+F3; also Enter and Shift+Enter in the find bar), Alt+Enter for full screen (F11), Ctrl+Alt+Q for Quit (Alt+F4). Menus show the F-key-free key; the palette shows all.

**Checks** ([TESTING.md](TESTING.md#2026-10-01--m1-build-ci-headless-self-tests-and-preview)): `STET_CI_DISPLAY=21 bash tools/ci.sh --ui` passed: fmt, clippy `-D warnings`, 139 tests, the build, the smoke run and 346 self-test steps in six scripts. The preview script wrote three screenshots.

**Package** ([TESTING.md](TESTING.md#2026-10-01--m1-package-makepkg-contents-and-a-sandboxed-install)): from commit `5a29660`, `tools/package-source.py` gave a reproducible tarball (the same SHA-256 twice) and `makepkg --cleanbuild --force --nodeps` in a scratch copy built `stet-0.0.1-1-x86_64.pkg.tar.zst`, with `check()` passing (116 tests). `--nodeps` because this machine's Rust comes from rustup, which pacman cannot see. The package holds the binary, the desktop file, the icons, the license bundle and two docs. In a sandboxed user-local install (scratch `XDG_DATA_HOME` and XDG state, cache and config), `gtk4-launch io.github.pixdevsapps.Stet` and `gio launch` started the release app headless, and GTK resolved both icons. Headless S6 with the release binary: `first-frame` median 76.6 ms with the built-in palette and 97.4 ms with the real Omarchy theme (M0's empty window: 56.4 ms), because the first window now waits for the theme and font.

**Preview (headless):** [editor, tokyo-night](../tools/progress/public/m1-editor-tokyo-night-2026-10-01.png), [palette, catppuccin-latte](../tools/progress/public/m1-palette-catppuccin-latte-2026-10-01.png), [close prompt, gruvbox](../tools/progress/public/m1-close-dialog-gruvbox-2026-10-01.png). The gate is judged after the live checks below.

**Deviations and findings:**
- No separate mock gate before the UI work: under the standing authorization the layout was built directly, and the screenshots above stand in for the mock.
- **Recent files:** the PRD's 1.x list names "`recent.json`" next to an omarchy-shell plugin, which reads as a file for such a plugin. M1's `recent.json` is Stet's own list (`schema_version` 1), not an interface for other programs; M2 may fold it into `session.json`. Files are added when opened or saved.
- **Status bar EOL** shows the line ending found at load instead of a static "LF": the buffer keeps line endings verbatim until M3, so a CRLF file saves as CRLF, but lines typed into it get LF.
- **Quitting and closing the window ask** about unsaved changes (Save All, Discard All, Cancel) until M2 makes them prompt-free.
- **Upstream and toolkit findings:** `sourceview5`'s async find returns uninitialised iterators when nothing matches ([ADR-005 amendment](DECISIONS.md#adr-005-amendment--find-next-through-our-own-async-wrapper-2026-10-01), added to the [upstream draft](upstream/sourceview5-replace-all.md)); GtkSourceView rewrites its pixel tab stops on every CSS change, and raises its search-match tag above ours on every scan (both handled in the window's layout phase).
- Ctrl+0 (Reset Zoom) is also the familiar key for "next Find Mark style"; M7 decides.
- `desktop-file-validate` hints that `Utility;TextEditor;Development;` has two main categories; kept as planned.
- **S4 correction:** `\` is unshifted on the Norwegian layout.

**Still to check in the live Hyprland session** (M1 exit criteria and carried-over items):
1. `omarchy theme set` across tokyo-night, catppuccin-latte and gruvbox and back, then `omarchy-theme-refresh`, with a document, the find bar and the palette open: the whole UI recolours live, with no flash, and light themes are legible in popovers and dialogs.
2. `omarchy-font-set "<another installed monospace font>"` (this machine has no `~/.config/fontconfig/` yet, so it is created): the editor font changes live and tab-indented columns stay aligned; then set it back.
3. Drag files from Nautilus onto the text, the tab strip and the status bar: each opens as a tab, and no URI text is inserted; dragging selected text inside the editor still moves it.
4. SUPER+V pastes into the editor and the find entry; SUPER+C and SUPER+X too.
5. With Stet on another workspace, `stet a.rs b.txt:40` from a terminal (and through `hyprctl dispatch 'hl.dsp.exec_cmd("…")'`): focus moves to Stet, both files open, the cursor is on line 40.
6. F-key-free keys reach the app: Ctrl+Shift+P, Alt+Down and Alt+Up (editor and find bar), Alt+Enter, Ctrl+Alt+Q; and F1, F3, F11 with Fn.
7. Norwegian keys: Ctrl++, Ctrl+-, Ctrl+0 and Ctrl+scroll zoom; Ctrl+Shift+Tab.
8. The package: build it from the final commit (`python3 tools/package-source.py && cd .local/package && makepkg --cleanbuild --force --nodeps`; `--nodeps` only because Rust comes from rustup here), `sudo pacman -U stet-0.0.1-1-x86_64.pkg.tar.zst` (the user's sudo), then the launcher entry and icon in the Omarchy menu, `gtk-launch io.github.pixdevsapps.Stet`, `hyprctl clients` showing the class `io.github.pixdevsapps.Stet`, and `pacman -Qkk stet` clean.
9. Open and Save As file choosers float and work.
10. Ctrl+W on a dirty tab and SUPER+W with dirty tabs both ask; S6 start time with the release build.

**Follow-ups:**
- P0 (M1 exit): the live checks above.
- P2: the per-theme `stet.toml` override (ADR-004) and S3's contrast guards are not built.
- P2: file the upstream issues (the user's action).
- P3: self-tests wait for real frames at Broadway's ~1 frame per second without a client; S1's web client would speed them up if the suite grows.

## M0 closed, and the whole plan authorized — 2026-10-01

The user answered the M0 questions in chat on 2026-10-01:

- **M0 is closed with GO for GTK4 + libadwaita + GtkSourceView 5** ([ADR-002](DECISIONS.md#adr-002--gtk4--libadwaita--gtksourceview-5-with-a-gpui-fallback-2026-09-30)). The preview gate ([screenshot](../tools/progress/public/m0-window-2026-09-30.png)) is approved.
- **Checks that weren't completed move to later milestones:**
  - drag-and-drop from Nautilus and SUPER+V into M1's exit criteria, with the real app;
  - fcitx5 typing in a column caret stays in M6's exit criteria;
  - compose and dead keys, plus fractional scaling at 1.25 and 1.6 (Omarchy's steps), go into the manual acceptance list in [TESTING.md](TESTING.md).
- **F-keys:** the user's keyboard sends media and brightness keys on the F-row unless Fn is held, and Hyprland consumes those. Every F-key action therefore needs an F-key-free alternative. The keys are chosen in M1 and recorded as an [ADR-009 amendment](DECISIONS.md#adr-009--classic-editor-keymap-with-omarchy-remaps-2026-09-30).
- **The commit of the M0 live-check work is authorized.**
- **Standing authorization:** "continue until you are finished with the whole plan, you do not need to ask me for permissions, I trust your judgement. Do all the testing yourself, you have full access to the computer." From 2026-10-01 this covers:
  - commits on `main`;
  - local package builds;
  - testing in the live session;
  - user-local installs for testing;
  - preview gates, which are now judged by the agent and recorded with screenshots.

  It does not cover what the agent cannot or should not do alone:
  - a system-wide `pacman -U`, which needs the user's sudo password;
  - pushing, or creating a remote or GitHub repository; the plan needs neither;
  - changing the user's real default editor or MIME associations, which is tested with sandboxed XDG directories instead.

The current milestone is **M1**.

## M0 live checks on Hyprland — 2026-09-30

At the user's request ("run the live checks"), spike S4 and live re-measures of S1, S5 and S6 ran in the user's Hyprland session (3440×1440 at 144 Hz, scale 1, keyboard layout `no`, fcitx5 with `keyboard-us`), with release builds. Full report: [spikes/S4-live.md](spikes/S4-live.md).

| Check | Verdict |
| --- | --- |
| S6 start, live | **Pass:** 224.5 ms median from `main` (273.6 ms from spawn); 346.3 ms with a 10 MB file |
| Second launch focuses the running window (from a shell and via compositor exec) | **Pass**; the files open as tabs in one window |
| File dialog floats | **Pass** (xdg-desktop-portal-gtk, 875×600) |
| Planned shortcuts against Hyprland binds | No conflicts (82 shortcuts, 226 binds) |
| fcitx5 grabs Ctrl+Shift+U and Ctrl+Alt+Shift+U | Confirmed |
| Ctrl+Space, Ctrl+D, Ctrl+Q, Ctrl+Enter, Ctrl+Shift+Up, Alt+Shift+Down, Ctrl+Tab | Reach the app |
| Typing through fcitx5; real-keyboard latency | **Pass**; key to painted frame 2.74 ms median, 14.4 ms p95 |
| SUPER+C / SUPER+V | Copy **passes**; paste not verified |
| S1 typing, live | Settled typing **passes** (2.70–5.43 ms median, p95 ≤ 12.10 ms), including offset 0; right after load it **fails** (70–77 ms median), confirming [ADR-014](DECISIONS.md#adr-014--syntax-highlighting-size-policy-2026-09-30) |
| S5 column mode, live | Same as headless; 10k-line per-line undo takes 238 ms, so the bulk-edit rule is needed |
| F-keys, drag-and-drop from Nautilus, SUPER+V, fcitx5 in a column caret, compose and dead keys, fractional scaling | **Not completed** in the two interactive sessions |

**Findings for later milestones:**
- **Hyprland 0.56 takes Lua in `hyprctl dispatch`** (`hl.dsp.focus`, `hl.dsp.exec_cmd`, `hl.dsp.window.close`). The first driver's old-style dispatches were rejected, and that result was discarded and re-run. [INTEGRATIONS.md](INTEGRATIONS.md) now documents the forms.
- **Startup is about 4× the Broadway figure** (GPU renderer start-up), still inside the bar. Keep S6 in each milestone's checks.
- **The user's layout is Norwegian:** `=`, `/`, `\` and brackets are shifted or AltGr keys, so M1's keymap audit must cover them.
- **Omarchy's scaling steps are 1, 1.25, 1.6 and 2,** not 1.5.

**Preview gate:** [tools/progress/public/m0-window-2026-09-30.png](../tools/progress/public/m0-window-2026-09-30.png) shows the M0 window with three tabs opened by three launches. It still has default libadwaita styling; the Omarchy theme arrives in M1. It awaits the user's approval.

**Proposed, awaiting user review:**
- Close M0 with **GO** for the GTK stack ([ADR-002](DECISIONS.md#adr-002--gtk4--libadwaita--gtksourceview-5-with-a-gpui-fallback-2026-09-30)).
- Move the checks that weren't completed into the milestones that build those features:
  - drag-and-drop and SUPER+V into M1's acceptance, with the real app;
  - fcitx5 in a column caret into M6;
  - compose and dead keys, plus fractional scaling at 1.25 and 1.6, into the manual acceptance list.
- Make M1's keymap audit cover the Norwegian layout.

**Open question for the user:** does the keyboard's F-row send F1–F12 without Fn? No F-key event reached the app in either session. If the row sends media keys by default, Hyprland consumes them, and the classic editor keymap's defaults (F3, F2, F7, F8, F11, F1) need an F-key-free path.

**Also changed:**
- `s1_perf` connects its Broadway web client only under Broadway, so it can run live.
- A new interactive harness, `s4_live`, is in the spikes crate.
- The preview screenshot is in `tools/progress/public/`.
- **A flaky test was fixed.** CI's first run after these changes failed once in `stet-infrastructure` (`uses_the_script_output_when_it_succeeds`), and stress runs under load reproduced it (1 failure in 400). The cause is ETXTBSY: a parallel test's forked child inherits the open write handle of the script another test has just written, so executing that script fails. The three tests that write or spawn executables now share a lock; the same stress then gave 0 failures in 800 runs.
- **Checks:** afterwards (on 2026-10-01, just after midnight), `INSTA_UPDATE=no bash tools/ci.sh --ui` passed: fmt, clippy `-D warnings` (including `s4_live`), 67 tests, the build, and the headless smoke run (`first-frame 56.1`).
- Then `target/` and `.local/` were removed again to free disk space. Nothing was committed; further commits are not authorized.

## Name, repository and spike changes approved — 2026-09-30

The user made these decisions in chat on 2026-09-30, on the results of the M0 spikes.

**Name: Stet** ([ADR-010](DECISIONS.md#adr-010-accepted--stet-2026-09-30), now Accepted).

- The proofreading mark "stet" (Latin for "let it stand") cancels a correction, which fits the core promise of never losing your text.
- Availability, checked on 2026-09-30: no `stet` package in the Arch repositories or the AUR, and no `stet` command on the system. A `stet` crate exists on crates.io; that does not matter, because our crates are `publish = false`. **Not checked:** trademarks, and GitHub organisation or repository names.
- Renamed: the binary and package `stet`; the crates `stet`, `stet-domain`, `stet-infrastructure` and `stet-spikes`; the app-ids `io.github.pixdevsapps.Stet` and `io.github.pixdevsapps.Stet.Devel`; the `STET_*` environment variables; the `stet:*` style ids; the rectangular clipboard type; the XDG directories; and the per-theme override `stet.toml`. ADR-010 has the full table.
- The project folder stays `/home/fredrick/Projects/linux-apps/notes`. "Notes" was the working name during research and M0; the spike reports and the entries below keep it.

**Repository.**

- `git init` of a local repository, on branch `main` as in Hertz, was authorized and **performed on 2026-09-30** (`git init -b main`; see the [authorization log](#authorization-log)). `.gitignore` also ignores insta's pending snapshots (`*.snap.new`, `*.pending-snap`).
- The **initial commit** was authorized separately at 23:11 the same day and made on `main`; it contains the M0 work, including this entry. No further commit, push, remote or GitHub repository (`PixDevsApps/Stet`) is authorized yet.

**Spike-driven changes, all seven approved:**

1. **PCRE2 everywhere, amended** ([ADR-005](DECISIONS.md#adr-005-amendment--our-own-replace-engine-2026-09-30)), from S2. SearchContext stays for highlighting, counting and Find Next. Replace, Replace All, Replace All in open documents and Replace in Files use our own PCRE2 matching (the `pcre2` crate) on a snapshot, with our own expansion of Boost-style replacement templates, applied through the bulk-edit path. A pattern-translation layer handles Boost `\<` `\>`, `\K`, and patterns that can match empty text.
2. **Syntax-highlighting size policy** ([ADR-014](DECISIONS.md#adr-014--syntax-highlighting-size-policy-2026-09-30), new), from S1. Highlighting switches off above a threshold in bytes and lines, and/or starts after a short delay, instead of staying on up to 50 MB. An S1 follow-up measurement in M3 sets the default; the working assumption is roughly 10–20 MB.
3. **The buffer is the source of truth, amended** ([ADR-003](DECISIONS.md#adr-003-amendment--bulk-edits-by-edit-count-2026-09-30)), from S2 and S5. A bulk edit is used above about 2,000 individual edits, decided by edit count. `max-undo-levels` was measured as 200, not 0; the explicit cap is set in M1.
4. **Exact tab stops in Pango units** ([ADR-015](DECISIONS.md#adr-015--exact-tab-stops-in-pango-units-2026-09-30), new), from S5: in M1, with a headless test.
5. **Column-mode editing and undo model** ([ADR-016](DECISIONS.md#adr-016--column-mode-editing-and-undo-model-2026-09-30), new), from S5. A typing burst is one undo group, closed before anything else acts; the fallback is one step per keystroke. Large rectangles use the bulk-edit rule, and the rectangular clipboard type is `application/x-stet-column-block`.
6. **Follow the live Omarchy theme, amended** ([ADR-004](DECISIONS.md#adr-004-amendment--reload-trigger-for-a-missing-current-directory-2026-09-30)), from S3. The reload also fires when `current` appears in `~/.local/state/omarchy/` (created, moved in, or renamed into place), for a watch that starts before `current/` exists.
7. **Upstream bug in `sourceview5` 0.11.2**: `SearchContext::replace_all` panics when nothing is replaced, and discards the count; `SearchContext::replace` asserts on a range that is not a match (debug builds panic, release builds silently replace nothing). Both were verified with a headless repro on 2026-09-30. It is recorded in the ADR-005 amendment, with a draft issue in [upstream/sourceview5-replace-all.md](upstream/sourceview5-replace-all.md). Filing it is the user's action; production code uses neither method.

[ADR-002](DECISIONS.md#adr-002--gtk4--libadwaita--gtksourceview-5-with-a-gpui-fallback-2026-09-30)'s status now says that the headless M0 spikes support the stack. **M0 is not closed:** the live checks below remain.

**Proposed, awaiting user review.** These spike recommendations are **not** approved; do not treat them as decisions.

- **Loading** (S1 recommendations 1–3): pace chunked loading with a timer instead of an idle (100 MB loaded in 1.04 s with a 16 ms timer, 40 s with idles); stream chunks from the loader thread instead of holding the whole string on the GTK thread; never load a large file with a single `set_text`. Noted under M3.
- **Long lines** (S1 recommendation 5): drop the "Open anyway" choice from the long-line prompt, or make it insert with hard breaks.
- **Keystroke bar** (S1 recommendation 9): restate it as "processing (insert to painted frame) p95 < 16 ms", plus a manual end-to-end check on Hyprland.
- **Replace All bars** (S2 recommendation 6): add "GTK thread blocked ≤ 2 s", and "undo of that Replace All ≤ 2 s, with RSS growth ≤ 2× the file size".
- **Headless harness** (S1 recommendation 10): move the Broadway web client (`connect_web_client` in `s1_perf.rs`) into the `stet-spikes` library or `tools/`, for the other spikes and the M1 `--self-test`.
- Smaller implementation notes stay in each report's "Recommendations" section, for the milestone that implements them: for example S3's contrast guards, S1's offset-0 investigation and S5's column-semantics proptests.

**Disk space.** The user asked to remove everything that is not needed. Done on 2026-09-30, after the rename was verified: `cargo clean` removed `target/` (11,046 files, 6.2 GiB, including the generated fixtures and S2's tables; about 2 GB on disk, because `/home` is btrfs with zstd compression), and `.local/` was deleted (1.4 MB of raw spike logs, JSON and PNGs). Stale Broadway sockets in `$XDG_RUNTIME_DIR` and a stale private D-Bus socket in `/tmp` from the headless runs were removed too, and `tools/headless.sh` now deletes Broadway's socket on exit. The repository is 952 KB. The spike reports keep every recorded number. To regenerate raw output, run `python3 tools/gen-fixtures.py` and re-run the spike ([BUILD.md](BUILD.md#spikes-m0)).

**Also documented:** why `tools/headless.sh` runs its own D-Bus configuration. On 2026-09-30 a plain private `dbus-run-session` auto-started xdg-desktop-portal-hyprland, which connected to the live Hyprland session; `tools/headless-dbus.conf` gives a bus that cannot start services ([BUILD.md](BUILD.md#headless-gtk-runs), [AGENTS.md](../AGENTS.md#running-checks)).

**Remaining for M0:**

- **S4 checks by the user** in the live Hyprland session: fractional scaling at 1.25 and 1.5, fcitx5 typing, key reachability (including Ctrl+Shift+U), drag-and-drop, floating file dialogs, activation focus and `SUPER+C/V`.
- **Live keystroke-to-paint re-measure** on Hyprland for S1 typing, the post-open stall and S5 column typing.
- **fcitx5 check in column mode:** preedit and commit, and compose keys (S5 row 25).
- **S6 by hand:** a true cold start and a live-Wayland start. `--wait` follows in M2.
- **Preview gate:** screenshots in `tools/progress/` and the user's approval.

**Checks.** After the code rename, `INSTA_UPDATE=no bash tools/ci.sh --ui` passed on 2026-09-30: fmt clean, clippy `-D warnings` clean (all spike binaries included), 67 tests passed with the renamed snapshots matching as they were (none re-accepted), the workspace built, and the headless smoke run logged `first-frame 55.5` from `stet::window`. A headless repro verified the upstream draft in debug and release builds ([upstream/sourceview5-replace-all.md](upstream/sourceview5-replace-all.md)). The checks in the entry below ran under the working name, before the rename.

**Docs changed:** DECISIONS (the ADR-002 status; amendments to ADR-003, ADR-004 and ADR-005; ADR-010 accepted; the new ADR-014, ADR-015 and ADR-016), this file, PRD, DESIGN, BUILD, TESTING, OPERATING, INTEGRATIONS, RESEARCH, README, AGENTS, CONTRIBUTING and LICENSE; a note under the title of each spike report; and the new [upstream/sourceview5-replace-all.md](upstream/sourceview5-replace-all.md).

## M0 foundation and headless spikes — 2026-09-30

> Written under the working name Notes, before the decisions in the entry above. The crate names and `NOTES_*` variables below are the ones used at the time (now `stet*` and `STET_*`, [ADR-010](DECISIONS.md#adr-010-accepted--stet-2026-09-30)). Its "Remaining for M0" and "Spike follow-ups" lists are superseded by the entry above.

**What exists.** Everything below was checked on 2026-09-30 on the development machine (Ryzen 9 5900X, gtk4 4.22.4, libadwaita 1.9.3, gtksourceview5 5.20.0, rustc 1.98.1).

- **Workspace** as in plan §3.1: `notes` (`app/`), `notes-domain` (`domain/`), `notes-infrastructure` (`infrastructure/`) and `notes-spikes` (`spikes/`); edition 2024, resolver 3, `default-members = ["app"]`, `publish = false`. `notes-domain` depends only on `memchr`; `notes-infrastructure` depends on `notes-domain`, `tracing` and `tracing-subscriber`, with no GTK or GLib; only `notes` and `notes-spikes` link GTK.
- **App** (M0 shell): an `AdwApplicationWindow` with an `AdwTabBar` in the header bar and one GtkSourceView per tab. Files given on the command line open on the GTK thread as lossy UTF-8 (the loader arrives in M3). Quit, new tab and close tab are wired through `ActionId`. The app logs `first-frame <ms>` and honours `NOTES_EXIT_AFTER_MS`. The app-id is `.Devel` in debug builds.
- **Domain:** `actions.rs` (`ActionId`), `text/eol.rs`, and `theme.rs` (palette, scheme XML and CSS, from S3). **Infrastructure:** `logging` (tracing to stderr), `omarchy/paths` and `omarchy/colors` (the `omarchy-theme-color` call with a Rust fallback, from S3).
- **Tools:** `tools/ci.sh`, `tools/headless.sh` with `tools/headless-dbus.conf`, and `tools/gen-fixtures.py`.
- **Docs:** README, AGENTS, CONTRIBUTING, LICENSE, and PRD, PLAN, DECISIONS (ADR-001 to ADR-013, including the M0 drafts 001, 002, 003, 004 and 010), DESIGN, RESEARCH, BUILD, TESTING, OPERATING and INTEGRATIONS, plus the spike reports in `docs/spikes/`.
- **Checks:**
  - `bash tools/ci.sh` passed: fmt, Clippy with `-D warnings` over all targets including the four spike binaries, 67 tests (app 1, domain 42, infrastructure 16, spikes library 5, `s2_search` 3) and the workspace build.
  - `bash tools/ci.sh --ui` passed: the debug app logged `first-frame 56.9` on Broadway `:9` and quit after 1.5 s.
  - `tools/headless.sh 9 env NOTES_EXIT_AFTER_MS=1500 cargo run -q -p notes -- Cargo.toml` logged `first-frame 80.8` and exited 0. The tab's contents were not asserted.
  - No `gtk4-broadwayd` was left running afterwards.

**Spike verdicts.** All numbers are headless runs under GTK Broadway (software rendering) on a shared machine. Every report lists its caveats.

| Spike | Verdict | Report |
| --- | --- | --- |
| S1 Performance | **Mixed.** The 100 MB first screen passes: 126 ms for plain text and 147 ms with Rust highlighting, using timer-paced 4 MB chunks, against ≤ 1.5 s. RAM passes only with highlighting off: 2.67× settled and 3.11× peak, but 7.04× with highlighting. Typing passes once highlighting has settled (7.7 ms median). It fails at offset 0 (29.5 ms) and in the ~7 s after opening a 10 MB Rust file (80–104 ms median; no paint for up to 2.05 s). The formatted long-line path passes (longest block 64–67 ms); a raw insert blocks for ≥ 89 s. Snapshots take 8.1 ms at 10 MB and 27.7 ms at 50 MB, so ADR-003 stands. | [S1-performance.md](spikes/S1-performance.md) |
| S2 Search | **Partial fail.** `SearchContext::replace_all` takes 8.0 s for 1M replacements, against ≤ 5 s, and blocks the GTK thread. Undoing it with the view attached takes 121 s and +8.2 GiB RSS. Replace-all is exactly one undo step (pass). `\U\1\E` works (pass), but the literal `\U$1` is inserted as typed. 48 of 52 PCRE2 probes behave correctly. | [S2-search.md](spikes/S2-search.md) |
| S3 Theme | **Pass.** Schemes load for 22/22 built-in themes (28/28 with user themes), with 0 CSS parse errors. One window switched live 140 times, including 5 light themes, and all sampled surfaces were correct. The reload trigger fires once per swap after the 150 ms debounce. A `CREATED current` trigger must be added. | [S3-theme.md](spikes/S3-theme.md) |
| S4 Wayland/Hyprland | **Not run.** It needs the live session. | — |
| S5 Column mode | **Feasible.** A column insert over 10k lines takes 29 ms, against < 200 ms, and is one undo step. Typing, Backspace and paste are replicated. Three things fail as-is and each has a measured fix: whole-pixel tab stops, one undo step per typed key, and ~250 ms undo with the view attached. fcitx5 was not run. | [S5-column.md](spikes/S5-column.md) |
| S6 Startup and `--wait` | **Startup passes, on a warm cache only:** 56.4 ms from `main` to the first frame and 109 ms from `exec`, against ≤ 400 ms. Second-launch forwarding takes 60.9 ms, against ≤ 100 ms, but the forwarded tab was not checked. `--wait` was **not tested**, because it arrives in M2. | [S1-performance.md, section e](spikes/S1-performance.md#e-s6-startup-5-runs-each-release-build) |

**Go/no-go inputs for the user** (not decisions):

- **S1** missed bars, so the M0 exit criteria call for a large-file ADR. The report proposes:
  - pacing chunked loading with a timer instead of an idle;
  - never loading a large file with a single `set_text`;
  - a highlighting cutoff by line count;
  - never inserting long lines raw, and dropping "Open anyway".
- **S5** is feasible, so the question "column mode in 1.0, or GPUI" does not reopen.
- **S2** is a design correction inside GTK, not a toolkit question. The report proposes amending ADR-005 so that Replace All uses our own PCRE2 matching and template expansion, applied as a bulk edit.

**Remaining for M0:**

- **S4 checks by the user** in the live Hyprland session: fractional scaling at 1.25 and 1.5, fcitx5 typing, key reachability (including Ctrl+Shift+U), drag-and-drop, floating file dialogs, activation focus and `SUPER+C/V`.
- **Live keystroke-to-paint re-measure** on Hyprland for S1 typing, the post-open stall and S5 column typing. The Broadway numbers leave out the compositor's frame wait.
- **IME check in column mode:** fcitx5 preedit and commit, and compose keys (S5 row 25).
- **S6 by hand:** a true cold start and a live-Wayland start. `--wait` follows in M2.
- **The name and app-id decision** ([ADR-010](DECISIONS.md)), an exit criterion.
- **Authorization for `git init`**, and optionally a private `PixDevsApps/<Name>` repository. None is recorded.
- **Spike follow-ups awaiting the user's decision:**
  - amend ADR-003: set the bulk threshold by edit count, and correct `max_undo_levels`, which read 200 in S2 rather than 0;
  - amend ADR-005: our own Replace All engine and pattern translation;
  - amend ADR-004's reload trigger;
  - the large-file ADR;
  - restate the keystroke bar as processing time plus a manual Hyprland check;
  - move S1's Broadway web client (`connect_web_client`) into the spikes library or `tools/`, so the other spikes and the M1 self-test can use it.
- **Preview gate:** screenshots in `tools/progress/` and the user's approval.

## Plan approved — 2026-09-30

The user approved the implementation plan. It came from a five-agent research workflow (feature usage, Omarchy 4.0.4 internals, Rust toolkits, core crates, prior art), an architect draft, and two adversarial critiques (technical and scope) whose corrections are folded in. See [RESEARCH.md](RESEARCH.md).

User decisions recorded on 2026-09-30:

- **Theme:** follow the live Omarchy theme and font ([ADR-004](DECISIONS.md)).
- **UI:** minimal hybrid: tabs, status bar, a hamburger menu with submenus in a familiar structure, and an F1 command palette. No toolbar ([DESIGN.md](DESIGN.md)).
- **Stack:** GTK4 + libadwaita + GtkSourceView 5 through gtk4-rs, gated by the M0 spikes ([ADR-002](DECISIONS.md)).
- **Keymap:** the classic editor keymap, with Omarchy remaps ([ADR-009](DECISIONS.md)).
- **1.0 scope:** lean 1.0 ([ADR-013](DECISIONS.md)).
- **Project memory:** repository docs only; no Notion ([ADR-012](DECISIONS.md)).
- **Name:** decided later, but before M0 ends ([ADR-010](DECISIONS.md)).

The approval covers the plan. It does not authorize `git init`, commits, pushes, repository creation, package builds or installs; those are recorded separately in the authorization log below.

## Execution model

Stet is delivered in explicit milestones. Sizes are focused engineering days.

Rules:

- keep the application runnable after each meaningful slice;
- keep work in progress small;
- every milestone ends with a **preview gate**: screenshots in `tools/progress/`, a dated entry in this file, and the user's approval;
- record decisions with long-term cost as ADRs in [DECISIONS.md](DECISIONS.md);
- every `git init`, commit, push, package build or install needs a dated user authorization in the log below before it happens;
- never invent acceptance evidence; record what was run, when, and its caveats;
- record follow-ups in this file instead of hiding TODOs in code;
- do not expand 1.0 scope because an idea is interesting (see Scope control).

## Current project state

- Product definition: plan approved 2026-09-30; [PRD](PRD.md) written.
- Name: **Stet**, decided 2026-09-30 ([ADR-010](DECISIONS.md#adr-010-accepted--stet-2026-09-30)). Debug app-id `io.github.pixdevsapps.Stet.Devel`, release app-id `io.github.pixdevsapps.Stet`. "Notes" was the working name during research and M0; the project folder keeps it.
- Design direction: minimal hybrid chosen. The [DESIGN.md](DESIGN.md) mock and screenshot approval gate happens at the start of M1, before any UI work.
- Project memory: repository docs ([ADR-012](DECISIONS.md)).
- Repository: git on `main`. Since 2026-10-01 its remote `origin` is the GitHub repository `PixDevsApps/Stet`, with `main` and the release tags pushed ([entry](#github-repository-pixdevsappsstet--2026-10-01)), public since 2026-10-02 ([entry](#stet-134-the-repository-goes-public--2026-10-02)); each further push needs the user's authorization. Since 2026-10-02 the history starts at 1.3.4, whose tag `v1.3.4` is the only one ([entry](#the-history-starts-at-134--2026-10-02)).
- Current milestones: M0 was closed with GO on 2026-10-01 ([entry](#m0-closed-and-the-whole-plan-authorized--2026-10-01)). All feature milestones are built, checked headless and merged into `main`: **M1 — Themed shell, installable**, **M3 — Files done right** and **M4 — Search** in Wave A ([entry](#wave-a-merged-m3-and-m4--2026-10-01)); **M2 — Never lose work**, **M5 — Text toolbox and chrome** (the MVP, `v0.3.0`), **M6 — Column mode** and **M8 — Navigation and Replace in Files** ([entry](#m2-m5-m6-and-m8-merged-mvp-tagged-live-acceptance--2026-10-01)); and **M7 — Marking, split view and compare** ([entry](#m7-built-and-checked-headless-live-checks-pending--2026-10-01)). **M9 — Polish, QA and 1.0** is done ([entry](#m9-done-stet-100--2026-10-01)), and the user signed off on 2026-10-02 ([entry](#the-user-signs-off-install-steps-in-the-readme--2026-10-02)). **1.1.0** adds, at the user's request, a double-click on a tab to rename its file and Save and Save As… in the tab menu ([entry](#stet-110-double-click-a-tab-to-rename-save-in-the-tab-menu--2026-10-01)); **1.2.0** names untitled tabs without saving ([entry](#stet-120-names-for-untitled-tabs-without-saving--2026-10-01)); **1.3.0** puts the menu on the left ([entry](#stet-130-the-menu-on-the-left--2026-10-01)), and **1.3.1** opens it into the window ([entry](#stet-131-the-menu-opens-into-the-window--2026-10-01)). **1.3.2** makes each menu page only as wide as its items and keeps the menu inside the window, fills in About and makes the icon the note alone ([entry](#stet-132-a-narrower-menu-about-and-the-icon--2026-10-02)); **1.3.3** rewords Stet's descriptions ([entry](#stet-133-new-wording-for-stets-descriptions--2026-10-02)); with **1.3.4** the repository went public on 2026-10-02 ([entry](#stet-134-the-repository-goes-public--2026-10-02)).

M0 status on 2026-09-30:

- The documentation set exists: README, AGENTS, CONTRIBUTING, LICENSE, and PRD, PLAN, DECISIONS, DESIGN, RESEARCH, BUILD, TESTING, OPERATING and INTEGRATIONS under `docs/`, plus the spike reports in `docs/spikes/` and an upstream issue draft in `docs/upstream/`.
- The Cargo workspace (`stet`, `stet-domain`, `stet-infrastructure`, `stet-spikes`), logging, the M0 window with one GtkSourceView per tab, `tools/ci.sh`, `tools/headless.sh` and `tools/gen-fixtures.py` exist. `bash tools/ci.sh` and `bash tools/ci.sh --ui` passed on 2026-09-30 under the working name, before the rename (see [the dated entry](#m0-foundation-and-headless-spikes--2026-09-30)).
- Spike results: S1, S2, S3 and S5, plus the startup part of S6, were measured headless under GTK Broadway. S4 and the live re-measures of S1, S5 and S6 ran in the user's Hyprland session ([S4-live.md](spikes/S4-live.md)). All are in [docs/spikes/](spikes/), linked from [RESEARCH.md](RESEARCH.md#m0-spike-results). S6 `--wait` can only be tested after M2 adds D-Bus activation.
- Decisions on 2026-09-30 ([dated entry](#name-repository-and-spike-changes-approved--2026-09-30)): ADR-010 accepted (Stet); ADR-003, ADR-004 and ADR-005 amended; ADR-014, ADR-015 and ADR-016 added. ADR-002's status says that the headless spikes support the stack.
- M0: closed on 2026-10-01 with GO. The checks that weren't completed moved to M1 (drag-and-drop, SUPER+V), M6 (column-caret IME) and the manual acceptance list (compose, scaling at 1.25 and 1.6). A true cold start for S6 needs root to drop caches and was not run.
- Preview gate: M0 approved by the user on 2026-10-01 ([screenshot](../tools/progress/public/m0-window-2026-09-30.png)). Later gates are judged by the agent under the standing authorization and recorded with screenshots.

### Authorization log

| Date | Action | Authorization | Status |
| --- | --- | --- | --- |
| 2026-09-30 | `git init` of a local repository (branch `main`) | Authorized by the user in chat | Performed 2026-09-30 (`git init -b main`) |
| 2026-09-30 | Initial commit of the M0 work on `main` | Authorized by the user in chat (23:11) | Performed 2026-09-30: the repository's first commit (`git log --reverse`) |
| 2026-10-01 | Commit of the M0 live-check work | Authorized by the user in chat | Performed 2026-10-01 |
| 2026-10-01 | Standing authorization: finish the whole plan without asking, including commits, local package builds, live-session testing and user-local installs ([entry](#m0-closed-and-the-whole-plan-authorized--2026-10-01)) | Authorized by the user in chat | Active |
| 2026-10-01 | Push, a remote, the GitHub repository `PixDevsApps/Stet`, a system-wide `pacman -U`, changing the user's real default editor | Not covered (see the entry) | The system install needs the user's sudo password |
| 2026-10-01 | M1 commits on the M1 branch; a local `makepkg` of the M1 package in a scratch directory; a sandboxed user-local install test (scratch XDG directories, headless) | Covered by the standing authorization | Performed 2026-10-01 ([M1 entry](#m1-built-and-checked-headless-live-checks-pending--2026-10-01)) |
| 2026-10-01 | M3 commits on the M3 branch; headless builds, tests and measurements | Covered by the standing authorization | Performed 2026-10-01 ([M3 entry](#m3-built-and-checked-headless-live-checks-pending--2026-10-01)) |
| 2026-10-01 | M4 commits on the M4 branch; headless builds and tests | Covered by the standing authorization | Performed 2026-10-01 ([M4 entry](#m4-search-built-and-checked-headless-live-checks-pending--2026-10-01)) |
| 2026-10-01 | Wave A: the merge commits of M3 and M4 and the fix commits on the Wave A branch; headless builds and tests | Covered by the standing authorization | Performed 2026-10-01 ([entry](#wave-a-merged-m3-and-m4--2026-10-01)) |
| 2026-10-01 | M2 commits on the M2 branch; headless builds, tests and measurements (private test buses for D-Bus activation, as in Rules) | Covered by the standing authorization | Performed 2026-10-01 ([M2 entry](#m2-built-and-checked-headless-live-checks-pending--2026-10-01)) |
| 2026-10-01 | The live acceptance run in the user's Hyprland session (`tools/live/acceptance.py`: windows on the current workspace, injected keys and pointer, `omarchy-theme-refresh`); M2, M5 and M6 commits on their branches | Covered by the standing authorization | Performed 2026-10-01 ([entry](#live-acceptance-on-hyprland-wave-b-and-m6-under-way--2026-10-01)) |
| 2026-10-01 | M5 commits on the M5 branch; headless builds, tests and measurements; Set as Default Editor and Move to Trash tested only in sandboxed home and XDG directories | Covered by the standing authorization | Performed 2026-10-01 ([M5 entry](#m5-built-and-checked-headless-live-checks-pending--2026-10-01)); running Set as Default Editor for the user's real session stays the user's step |
| 2026-10-01 | M6 commits on the M6 branch; headless debug and release builds, tests and measurements | Covered by the standing authorization | Performed 2026-10-01 ([M6 entry](#m6-column-mode-built-and-checked-headless-live-checks-pending--2026-10-01)) |
| 2026-10-01 | M8 commits on the M8 branch; headless builds, tests and measurements (Replace in Files only in temporary folders; quick open reads `~/Projects` without writing) | Covered by the standing authorization | Performed 2026-10-01 ([M8 entry](#m8-built-and-checked-headless-live-checks-pending--2026-10-01)) |
| 2026-10-01 | The merges of M2, M5, M6 and M8 on `main` with integration fixes; annotated tags `v0.2.0` and `v0.3.0`; the live acceptance runs, including a temporary D-Bus service file for the test app-id in `~/.local/share/dbus-1/services/` (removed after the check), a terminal and a file manager opened and closed by the tab menu check, and a sandboxed trash under `~/.cache` | Covered by the standing authorization | Performed 2026-10-01 ([entry](#m2-m5-m6-and-m8-merged-mvp-tagged-live-acceptance--2026-10-01)) |
| 2026-10-01 | M7 commits on the M7 branch; headless debug and release builds, tests and measurements (displays 60–69) | Covered by the standing authorization | Performed 2026-10-01 ([M7 entry](#m7-built-and-checked-headless-live-checks-pending--2026-10-01)) |
| 2026-10-01 | M9: the M7 merge and fixes on `main`; version 1.0.0 and the annotated tag `v1.0.0`; `makepkg` of the 1.0.0 package and the fresh-install test in an `archlinux:base` Docker container (packages from the Arch mirrors); the live acceptance runs; removing the agents' merged worktrees and branches and Stet's test leftovers in `~/.local/state/stet` and `~/.cache/stet` | Covered by the standing authorization | Performed 2026-10-01 ([entry](#m9-done-stet-100--2026-10-01)) |
| 2026-10-01 | 1.1.0, at the user's request in chat (a double-click on a tab to rename; Save and Save As… in the tab menu): commits on `main`, version 1.1.0 and the annotated tag `v1.1.0`; `makepkg` and the fresh-install test in the container; the live checks `tab_double_click` and `tab_menu` | Covered by the standing authorization; the features are the user's requests | Performed 2026-10-01 ([entry](#stet-110-double-click-a-tab-to-rename-save-in-the-tab-menu--2026-10-01)) |
| 2026-10-01 | 1.2.0, at the user's request in chat (names for untitled tabs without saving): commits on `main`, version 1.2.0 and the annotated tag `v1.2.0`; `makepkg` and the fresh-install test in the container; the live checks `tab_double_click` and `tab_menu` | Covered by the standing authorization; the feature is the user's request | Performed 2026-10-01 ([entry](#stet-120-names-for-untitled-tabs-without-saving--2026-10-01)) |
| 2026-10-01 | 1.3.0, at the user's request in chat (the hamburger menu on the left): commits on `main`, version 1.3.0 and the annotated tag `v1.3.0`; `makepkg` and the fresh-install test in the container; the live checks `main_menu`, `tab_double_click`, `tab_menu`, `split_view` and `a11y_audit` | Covered by the standing authorization; the change is the user's request | Performed 2026-10-01 ([entry](#stet-130-the-menu-on-the-left--2026-10-01)) |
| 2026-10-01 | Create the GitHub repository `PixDevsApps/Stet` (private, like `PixDevsApps/Hertz`, as the approved plan proposed), add it as `origin`, commit the docs for it, and push `main` and the annotated tags `v0.2.0`–`v1.3.0` | Authorized by the user in chat: "Please update/create all necessary docs, create a new GitHub repo, "Stet", commit and push" | Performed 2026-10-01: created with `gh repo create --private`; pushed `main` at `2d8cd75` and the six tags, checked with `git ls-remote origin`; then this record ([entry](#github-repository-pixdevsappsstet--2026-10-01)) |
| 2026-10-01 | Create the GitHub release `v1.3.0` on `PixDevsApps/Stet` from the pushed tag, with the tested package `stet-1.3.0-1-x86_64.pkg.tar.zst` attached and its SHA-256 in the notes | Authorized by the user in chat: "yes, attach the 1.3.0 package to a GitHub release" | Performed 2026-10-01 ([entry](#github-release-v130--2026-10-01)) |
| 2026-10-01 | Push `main` with the commit that records the v1.3.0 release, this row included | Authorized by the user in chat: "yes, push it" | Performed 2026-10-01: `main` pushed with this record |
| 2026-10-01 | 1.3.1 (the menu opened half outside the window): commits on `main`, version 1.3.1 and the annotated tag `v1.3.1`; `makepkg` and the fresh-install test in the container | Covered by the standing authorization; pushing and a GitHub release need the user's authorization | Performed 2026-10-01 ([entry](#stet-131-the-menu-opens-into-the-window--2026-10-01)) |
| 2026-10-02 | 1.3.2, at the user's request in chat (a narrower main menu that opens inside the window; more details in About; the icon only the note, without its tile): commits on `main`, version 1.3.2 and the annotated tag `v1.3.2`; `makepkg` and the fresh-install test in the container; the live checks with the release build, with grim screenshots of Stet's window | Covered by the standing authorization; the changes are the user's requests; pushing and a GitHub release need the user's authorization | Performed 2026-10-02 ([entry](#stet-132-a-narrower-menu-about-and-the-icon--2026-10-02)); not pushed |
| 2026-10-02 | 1.3.3, at the user's request in chat (new wording for `stet --help`, the package description, the desktop entry's comment and the README's introduction): commits on `main`, version 1.3.3 and the annotated tag `v1.3.3`; `makepkg` and the fresh-install test in the container | Covered by the standing authorization; the change is the user's request; pushing, a GitHub release and the GitHub repository's description need the user's authorization | Performed 2026-10-02 ([entry](#stet-133-new-wording-for-stets-descriptions--2026-10-02)); before anything was pushed, the tag was moved once, to the commit with the `keys.toml` change |
| 2026-10-02 | 1.3.4: commits on `main`, version 1.3.4 and the annotated tag `v1.3.4`; `makepkg` and the fresh-install test in the container; edit the GitHub repository's description and the v1.3.0 release's notes. Push `main` and the annotated tags `v1.3.1`–`v1.3.4`; create the GitHub release for the newest version with its tested package; make `PixDevsApps/Stet` public | Authorized by the user in chat | Performed 2026-10-02 ([entry](#stet-134-the-repository-goes-public--2026-10-02)): 1.3.4; the description and the v1.3.0 notes edited; `main` and `v1.3.1`–`v1.3.4` pushed; the release v1.3.4 created; the repository public; then this record pushed |
| 2026-10-02 | Push `main` with only the commit that adds the README's install steps and records the user's sign-off, this row included | Authorized by the user in chat, choosing "Push only the README" when asked whether to push | Performed 2026-10-02 ([entry](#the-user-signs-off-install-steps-in-the-readme--2026-10-02)): `main` pushed with this record |
| 2026-10-02 | Rewrite the history from 1.3.1 on without files that don't belong in the repository and every mention of them (files, docs and commit messages), move the tags `v1.3.1`–`v1.3.4` with it, and force-push `main` and those tags, this row included | Authorized by the user in chat, choosing "Rewrite the history" when asked | Performed 2026-10-02 ([entry](#the-history-from-131-on-rewritten--2026-10-02)): rewritten with `git filter-branch`; force-pushed with this record |
| 2026-10-02 | Build the 1.3.4 package again from the rewritten `v1.3.4`, test it in the container, and replace the v1.3.4 release's package and the checksum in its notes | Authorized by the user in chat, choosing "Rebuild and replace it" when asked | Performed 2026-10-02 ([entry](#the-history-from-131-on-rewritten--2026-10-02)): built and tested before the force-push; the release changed after it |
| 2026-10-02 | Rewrite the whole history without the `Co-Authored-By: Claude` line in its commit messages, with the commit IDs the docs name mapped; move the tags `v0.2.0`–`v1.3.4`; force-push `main` and those tags, this row included; and give the new commit IDs in the v1.3.0 and v1.3.4 release notes | Authorized by the user in chat, choosing "Remove it everywhere" when asked whether to remove Claude as a contributor | Performed 2026-10-02 ([entry](#no-co-author-line-in-the-history--2026-10-02)): force-pushed with this record |
| 2026-10-02 | Switch the GitHub repository's default branch to a temporary branch at `main`'s commit and back to `main`, so that GitHub counts its contributors again; create and delete that branch | Authorized by the user in chat: "do the default branch switch now" | Performed 2026-10-02, 13:28:40–13:28:51: the branch `refresh` at `main`'s commit made the default, `main` the default again, `refresh` deleted; GitHub then recomputed the contributor statistics, and the repository page's Contributors box showed only PixDevsApps ([entry](#no-co-author-line-in-the-history--2026-10-02)) |
| 2026-10-02 | Start the history afresh at 1.3.4: a new `main` whose first commit holds the current files, the annotated tag `v1.3.4` on it, and a commit with the package's evidence; delete the tags `v0.2.0`–`v1.3.3` here and on GitHub; build the 1.3.4 package again from the new tag and test it in the container; force-push `main` and `v1.3.4`, this row included; delete the release v1.3.0; replace the v1.3.4 release's package and the checksum and commit in its notes | Authorized by the user in chat, choosing "Start history fresh" when asked | In progress |
| — | `makepkg`, `pacman -S`/`-U`, `cargo install` | Not authorized on 2026-09-30 | Since 2026-10-01 local package builds and user-local test installs are covered by the standing authorization; a system-wide `pacman -U` stays the user's step |

An authorization covers only what it names ([OPERATING.md](OPERATING.md#authorization-gates)).

## Milestone plan

Every milestone ends with a preview gate: screenshots in `tools/progress/`, a dated entry in this file, and the user's approval.

### M0 — Foundation and go/no-go spikes (8–12 days)

Goal: the workspace and docs skeleton exist, and the toolkit risks have measured answers.

Key outcomes:

- `git init` of a local repository (authorized 2026-09-30), and optionally a private `PixDevsApps/Stet` repository (not authorized yet);
- the workspace with AGENTS.md, `tools/ci.sh` and logging;
- a window with one GtkSourceView;
- drafts of ADR-001 (standalone app), ADR-002 (stack), ADR-003, ADR-004 and ADR-010 (name and app-id);
- the spikes below.

| Spike | What it checks | Pass bar |
| --- | --- | --- |
| S1 Performance | Open 100 MB / 1M lines | First screen ≤ 1.5 s, RAM ≤ 3× file size |
| | Typing in a 10 MB file | < 16 ms keystroke-to-paint |
| | 10 MB single-line JSON through the long-line path | No freeze over 2 s |
| | Snapshot cost | Measured at 10 MB and 50 MB |
| S2 Search | Regex replace-all over 1M lines | ≤ 5 s, one undo step |
| | Template replacements | `\U$1` works |
| S3 Theme | Scheme XML plus CSS variables | Recolours the whole UI; live switching across 3 themes, including catppuccin-latte (light) |
| S4 Wayland/Hyprland | Fractional scaling at 1.25/1.5; fcitx5 typing; key reachability; drag-and-drop; floating file dialogs; activation focus; `SUPER+C/V` | Each checked |
| S5 Column mode | Rectangle painted with `snapshot_layer`, plus fcitx5 typing, Backspace and paste replicated through `insert-text`/`delete-range`, with single-step undo over 10k lines | Feasible |
| S6 Startup and `--wait` | Cold start; D-Bus-activated `--wait` when Stet is **not** already running | Measured; `--wait` works |

Exit criteria:

- the spike report is in RESEARCH.md (results in `docs/spikes/`);
- **the name and app-id are final** (the backup path and D-Bus name depend on them). Met on 2026-09-30: Stet ([ADR-010](DECISIONS.md#adr-010-accepted--stet-2026-09-30));
- go/no-go, judged separately:
  - an S1 miss leads to an ADR for a large-file viewer. S1's highlighting misses led to [ADR-014](DECISIONS.md#adr-014--syntax-highlighting-size-policy-2026-09-30), approved on 2026-09-30, and S1 counts as passed or mitigated ([ADR-002 status](DECISIONS.md#adr-002--gtk4--libadwaita--gtksourceview-5-with-a-gpui-fallback-2026-09-30));
  - an S5 miss reopens the question "column mode in 1.0, or GPUI". S5 is feasible, so the question does not reopen.
- preview gate approved.

### M1 — Themed shell, installable (8–10 days)

Goal: an installable editor shell in the live Omarchy theme, where the action registry drives the menu and the palette.

Before any UI work: a DESIGN.md mock of the tab strip, status bar, find bar, palette and banners, **approved by the user**.

Key outcomes:

- the action registry, with the hamburger menu and F1 palette generated from it;
- tabs; new, open, save and close with UTF-8 only;
- recent files, drag-and-drop, syntax detection and a language picker;
- basic Ctrl+F find, go to line, wrap, whitespace and zoom;
- single instance with `file:line:col`;
- **the live Omarchy theme and font**, with the full reload trigger, including `current` appearing (created, moved in or renamed into place) for a watch that starts before `current/` exists ([ADR-004 amendment](DECISIONS.md#adr-004-amendment--reload-trigger-for-a-missing-current-directory-2026-09-30));
- exact tab stops in Pango units, re-applied on tab-width, font and zoom changes, with a headless test ([ADR-015](DECISIONS.md#adr-015--exact-tab-stops-in-pango-units-2026-09-30));
- the explicit undo cap, its value decided in M1 ([ADR-003 amendment](DECISIONS.md#adr-003-amendment--bulk-edits-by-edit-count-2026-09-30));
- a basic status bar;
- a `--self-test` harness that drives ActionIds;
- `PKGBUILD.in` and `package-source.py`.

Exit criteria:

- `stet a.rs b.txt:40` run from a second terminal opens both files in the running window at the right line;
- `omarchy theme set` recolours the app live, and a headless self-test shows that the reload also fires when `current/` appears after the app started;
- a headless test shows that the `iter_location` x after a tab lies on the character grid, also after a tab-width change;
- `makepkg` builds the package, and a user-local install gives a launcher entry that works. The system-wide `pacman -U` needs the user's sudo password.
- **Carried over from M0:** dropping files from Nautilus opens them, and SUPER+V pastes into the editor, both checked live;
- every F-key action has an F-key-free alternative ([ADR-009](DECISIONS.md#adr-009--classic-editor-keymap-with-omarchy-remaps-2026-09-30));
- preview gate recorded.

### M2 — Never lose work (6–8 days)

Goal: unsaved and untitled work survives quitting, crashes, logout and reboot, with no prompts.

Key outcomes:

- `session.json` and backups every 7 s, with the ordering guarantee;
- lazy restore, the conflict banner, orphan recovery and the lock;
- signal flushing;
- D-Bus activation with `--wait` and `--no-session`;
- settings to turn backups on or off and set the interval;
- the "Forget drafts" command.

Exit criteria:

- open 40 dirty and untitled tabs, then `kill -9`: everything comes back, with ≤ 7 s of edits lost;
- `SUPER+W`, `systemctl --user stop` on the app scope, and a reboot all restore without prompts;
- `GIT_EDITOR="stet --wait" git commit` works with Stet both running and not running;
- a 50-tab session is interactive in ≤ 500 ms;
- preview gate approved.

### M3 — Files done right (10–14 days)

Goal: any text file opens, edits and saves without silent data loss, and big files never freeze the UI.

Key outcomes:

- the encoding pipeline, including reinterpret and convert popovers, the NUL placeholder and the `lossy_roundtrip` flag;
- EOL handling and the Mixed banner;
- strict save with the "can't represent this character" dialog;
- safe save edge cases;
- binary files open read-only, and read-only files show the banner;
- large-file mode, the long-line path, and chunked loading with progress;
- the syntax-highlighting size policy ([ADR-014](DECISIONS.md#adr-014--syntax-highlighting-size-policy-2026-09-30)): an S1 follow-up measurement of right-after-load typing at 1, 2, 5 and 10–20 MB sets the default threshold in bytes and lines, and/or a delayed start; the formatted long-line view follows the same rule;
- reload banners.

Implementation notes from S1, **proposed and awaiting user review** (see the [2026-09-30 entry](#name-repository-and-spike-changes-approved--2026-09-30)): pace chunk inserts with a timer rather than an idle; stream chunks from the loader thread; never use a single `set_text` for a large file. Adopted in M3 under the standing authorization ([ADR-003 amendment](DECISIONS.md#adr-003-amendment--loading-and-reloading-files-2026-10-01)).

Exit criteria:

- every fixture either round-trips byte-for-byte or is flagged lossy when opened. The fixtures are UTF-16LE/BE with and without BOM, UTF-8 BOM, Windows-1252, CR-only, CRLF and NUL, plus the CJK files if the user wants them;
- saving through a symlink keeps the link;
- `git checkout` of a clean open file reloads it without switching tabs;
- a 200 MB log opens in large mode without freezing;
- the highlighting threshold is measured and recorded in ADR-014;
- preview gate approved.

### M4 — Search, tagged 0.2 (8–10 days)

Goal: a familiar search toolbox, with the same PCRE2 behaviour in the document and in Find in Files.

Key outcomes:

- the find/replace bar with all modes and options, count, in selection and history;
- smart highlight;
- Find All in the current document and in all open documents; Replace All in all open documents;
- our own replace engine: Replace, Replace All and Replace All in open documents match with the `pcre2` crate on a snapshot, expand Boost-style replacement templates (`$1`, `${1}`, `$&`, `\1`, `\U…\E`, …) in our own code, and apply the result through the bulk-edit path ([ADR-005 amendment](DECISIONS.md#adr-005-amendment--our-own-replace-engine-2026-09-30), [ADR-003 amendment](DECISIONS.md#adr-003-amendment--bulk-edits-by-edit-count-2026-09-30));
- the pattern-translation layer: Boost `\<` `\>`, `\K`, and patterns that can match empty text routed to our own matcher;
- no production use of `SearchContext::replace_all` or `SearchContext::replace` ([upstream bug](upstream/sourceview5-replace-all.md));
- Find in Files with its results panel;
- the PCRE2 parity suite.

Exit criteria:

- about 60 Boost-style patterns give the same results in-document and in Find in Files. They include `\R`, lookbehind, `\U$1`, and Extended `\r\n` in a CRLF document;
- Replace All over 1M lines takes ≤ 5 s and is one undo step, measured end to end through our engine;
- Find in Files over `~/Projects` shows first results in < 300 ms and cancels instantly;
- preview gate approved; tagging 0.2 needs the git authorization.

### M5 — Text toolbox and chrome, tagged 0.3, MVP complete (8–10 days)

Goal: complete the MVP: text tools, keymap and config files, the full chrome, and Omarchy default-editor integration.

Key outcomes:

- line, case and comment operations; prefix/suffix and incrementing numbers;
- JSON and XML tools, and brace jump;
- the classic editor keymap with `keys.toml`, and `config.toml` hot reload;
- tab and editor context menus; indentation settings; the full status bar; window title;
- the document-map toggle;
- the "Set as default editor" command;
- INSTALL.md and INTEGRATIONS.md.

Exit criteria:

- every MVP action works from its shortcut, the menu and the palette;
- formatting a 20 MB JSON file doesn't block the UI;
- after "Set as default editor", `SUPER+SHIFT+N` opens Stet and double-clicking a file in Nautilus opens it in Stet;
- preview gate approved; tagging 0.3 needs the git authorization.

### M6 — Column mode (8–12 days)

Goal: rectangular selection and editing, and the Column Editor.

Key outcomes:

- rectangular selection and editing with virtual space;
- column typing as one undo group per burst, closed before anything else acts; the fallback is one undo step per keystroke ([ADR-016](DECISIONS.md#adr-016--column-mode-editing-and-undo-model-2026-09-30));
- large rectangles through the bulk-edit rule ([ADR-003 amendment](DECISIONS.md#adr-003-amendment--bulk-edits-by-edit-count-2026-09-30));
- the rectangular clipboard as the custom MIME type `application/x-stet-column-block`, with a plain-text fallback;
- the Column Editor (Alt+C: text or numbers, start, step, repeat, leading zeros, base).

Exit criteria:

- inserting at column 0 across 10k lines is one undo step and takes < 200 ms;
- undo and redo of a 10k-line column edit each take < 200 ms, through the bulk-edit rule;
- a column typing burst undoes in one step (the per-keystroke fallback is documented in ADR-016);
- the column-math proptests pass;
- fcitx5 typing works in column mode;
- preview gate approved.

### M7 — Marking, split view and compare (10–14 days)

Goal: a familiar marking workflow, two views, and file compare.

Key outcomes:

- bookmarks and gutter; Mark with bookmarked-line operations; style tokens;
- split, clone and move view (F8); synchronized scrolling;
- compare, including against the clipboard: ignore whitespace and case, next/previous diff;
- the reload banner's Compare button.

Exit criteria:

- marking a regex with bookmark lines, then removing the unbookmarked lines, is one undo step;
- comparing two 10k-line files takes < 1 s;
- preview gate approved.

### M8 — Navigation and Replace in Files (6–8 days)

Goal: fast navigation between documents and positions, and project-wide replace.

Key outcomes:

- quick open, the MRU switcher, back and forward;
- pinned tabs and first-line tab names;
- Replace in Files, through the same engine as Replace All ([ADR-005 amendment](DECISIONS.md#adr-005-amendment--our-own-replace-engine-2026-09-30)); open documents receive the change as an undoable edit.

Exit criteria:

- Replace in Files keeps each file's encoding, BOM and EOL on all fixtures;
- preview gate approved.

### M9 — Polish, QA and 1.0 (6–8 days)

Goal: a release-quality 1.0 that the user signs off.

Key outcomes:

- the performance pass and an Orca accessibility check;
- USER_GUIDE with the keymap;
- the acceptance run ([TESTING.md](TESTING.md));
- a deterministic tarball and license bundle;
- a package upgrade test from 0.3, keeping sessions;
- a fresh-install test in a container.

Exit criteria:

- all checklist items pass or are recorded in RELEASE_STATUS;
- no P1 bugs;
- the user signs off.

**Totals:** about **48–64 days to the MVP (0.3)** and about **78–106 days to 1.0**.

## Work sequencing principles

Prefer vertical slices that keep the app runnable, but do not break the dependency order below.

1. Measure before committing: the M0 spikes gate the toolkit, and each go/no-go is judged on its own.
2. Theme and packaging early (M1). The live theme is the most visible "native to Omarchy" trait, and an installable build lets the user try every preview.
3. Never lose work before file edge cases (M2 before M3). The session model uses `#[serde(default)]`, so encoding and EOL fields can be added in M3 without a migration.
4. Search, then the text toolbox (M4, M5), completing the MVP at 0.3.
5. Column mode first after the MVP (M6), because it is the feature people cite most as one they rely on. Prefix/suffix and numbering tools cover the common cases in the MVP.
6. Then marking, split view and compare (M7), navigation and Replace in Files (M8), and polish (M9).

Within a milestone:

- land `domain` logic with its tests before the UI uses it;
- route every new command through an `ActionId` from the start;
- measure performance-sensitive paths before and after a change.

## Scope control

Do not implement before Stet 1.0. Add ideas to these lists instead of the code.

**1.x (after 1.0):**

- code folding, macros, multi-caret, Run (F5), tail -f mode;
- word completion, auto-close pairs;
- a save-as-administrator helper using pkexec;
- a Preferences GUI and keymap editor, and a VS Code-style preset;
- the full character-sets menu and OEM code pages;
- document list and folder workspace, print, `recent.json`, and an omarchy-shell plugin.

**Later:**

- function list and structural folding (tree-sitter);
- importing user-defined language (UDL) files (user `.lang` files work from day one);
- spell check;
- hex view;
- JSON tree view.

**Never:**

- plugin ABI or scripting;
- a Style Configurator (the theme drives colours);
- an FTP plugin, an auto-updater, always-on-top (not possible on Wayland), clipboard history;
- Markdown preview (Omawrite covers it), i18n for 1.0, named session files.

Moving an item between tiers is a user decision, recorded as a dated entry here and an amendment to [ADR-013](DECISIONS.md).

## Synchronization rule

The repository is the only project memory; there is no external tracker to mirror.

This file contains:

- the current status and active milestone;
- blockers and follow-ups;
- the authorization log;
- dated entries for progress and preview gates, newest first;
- the milestone structure, sequencing principles and scope control.

Other docs hold the details: decisions in [DECISIONS.md](DECISIONS.md), test evidence and the acceptance checklist in [TESTING.md](TESTING.md), spike results in [docs/spikes/](spikes/) (linked from [RESEARCH.md](RESEARCH.md)).

A change to milestones, scope, architecture or execution rules updates every affected doc in the same change, and moves the "Synced:" date.
