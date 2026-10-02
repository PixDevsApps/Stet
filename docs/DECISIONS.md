# Architecture Decisions

Project: Stet, a fast, keyboard-first text and code editor for Omarchy

Synced: 2026-10-02

The repository docs are the canonical project memory ([ADR-012](#adr-012--repository-docs-as-canonical-project-memory-2026-09-30)). Dated entries describe the state at that date; newer entries and amendments take precedence.

---

Use this page for decisions with meaningful long-term cost. Keep each entry short, dated, and explicit about alternatives. Change an accepted decision with a dated `###` amendment under it; do not rewrite history. Research references (HN, SO, issue-tracker requests, critique numbers) point to [RESEARCH.md](RESEARCH.md).

ADR-001 to ADR-013 were drafted on 2026-09-30 under the working name "Notes". [ADR-010](#adr-010--name-and-app-id-2026-09-30) was accepted the same day with the name **Stet**, so the product name, commands, paths and identifiers in these ADRs now read as Stet. ADR-010 itself keeps the working values as history.

| ADR | Title | Status (2026-10-02) |
| --- | --- | --- |
| 001 | Standalone native application | Accepted |
| 002 | GTK4 + libadwaita + GtkSourceView 5, GPUI fallback | Accepted (user decision); confirmed when M0 closed with GO on 2026-10-01 |
| 003 | The buffer is the source of truth | Accepted (user decision), as amended 2026-09-30; undo cap of 1,000 steps set 2026-10-01; loading and reloading in M3 (2026-10-01); text tools on workers and large results in pieces (M5, 2026-10-01); large column edits keep the view (M6, 2026-10-01); bookmarks and marks across bulk edits, Undo and Redo (M7, 2026-10-01) |
| 004 | Follow the live Omarchy theme | Accepted (user decision); reload trigger amended; built in M1 (2026-10-01); role colours turned apart when a theme can't tell them apart (M9) |
| 005 | PCRE2 everywhere | Accepted (user decision), as amended 2026-09-30 and 2026-10-01 (async Find Next wrapper; search as built in M4, with the widget parity run; search over M3's documents when M3 and M4 were merged; Replace in Files and its open-document policy in M8) |
| 006 | Plain-file session and backups | Proposed; one fingerprint type since 2026-10-01 (M3); built in M2 (2026-10-01); pinned tabs and first-line names kept (M8); views, clones, bookmarks and the divider kept (M7); untitled tabs' names kept (1.2) |
| 007 | Own encoding pipeline | Proposed; built in M3 (2026-10-01) |
| 008 | LF-normalized buffer | Proposed; built in M3, line-ending changes outside the undo history (2026-10-01) |
| 009 | Classic editor keymap with Omarchy remaps | Accepted (user decision); F-key-free keys and the Norwegian audit added 2026-10-01; search keys (M4); text-tool keys, editor-scoped keys, UPPERCASE on Alt+Shift+U and the overridden built-in bindings (M5); column-mode keys, Alt+Shift+arrows taken over as application accelerators (M6); navigation keys (M8: Ctrl+P, Ctrl+Tab as the switcher, Alt+Left/Right); marking, view and compare keys (M7: Ctrl+Alt+K/L/J beside the F2 keys, Ctrl+Alt+1–5 for Jump Up, Ctrl+Alt+O beside F8, the compare keys) |
| 010 | Name and app-id | Accepted (user decision): **Stet** |
| 011 | D-Bus activation for single instance and `--wait` | Proposed; single instance and the command line built in M1; activation, the development fallback and `--wait` built in M2 (2026-10-01) |
| 012 | Repository docs as canonical project memory | Accepted (user decision) |
| 013 | Lean 1.0 scope | Accepted (user decision); after 1.0, a double-click on a tab to rename and Save in the tab menu (1.1.0), and names for untitled tabs (1.2.0), user decisions 2026-10-01 |
| 014 | Syntax-highlighting size policy | Accepted (user decision); limits measured in M3: 2 MiB and 100,000 lines, no delayed start (2026-10-01); settable in config.toml (M5) |
| 015 | Exact tab stops in Pango units | Accepted (user decision); re-applied after GtkSourceView's resets (2026-10-01) |
| 016 | Column-mode editing and undo model | Accepted (user decision); built in M6 without the per-keystroke fallback, its open points settled (2026-10-01) |
| 017 | Settings in `config.toml` and `keys.toml` | Accepted — M5, under the standing authorization (2026-10-01) |
| 018 | Split view and compare presentation | Accepted — M7, under the standing authorization (2026-10-01) |

## ADR-001 — Standalone native application (2026-09-30)

**Status:** Accepted with the plan, 2026-09-30.

**Decision:** Stet is a standalone native desktop application with its own window. It is not an omarchy-shell plugin, a Quickshell widget, a terminal UI, or a Windows program running under Wine.

**Reason:**
- An editor needs a real text widget with input methods, accessibility, clipboard, drag-and-drop and file dialogs. omarchy-shell plugins run unsandboxed inside the shell process.
- A Windows program under Wine assumes 96 DPI (tiny text on HiDPI), has no native Wayland, uses Windows file dialogs and ignores the theme.
- The non-vim terminal demand is already met (`omarchy-launch-editor` supports fresh, nano and micro in a terminal; microsoft/edit exists). The gap is a GUI editor.
- Same shape as Hertz ADR-001: independent ownership and a predictable lifecycle.

**Consequence:** Stet owns its window, session, file I/O and desktop integration (desktop entry, D-Bus single instance). An omarchy-shell plugin (quick note or recent files) is at most a 1.x add-on that reads a file Stet writes.

## ADR-002 — GTK4 + libadwaita + GtkSourceView 5, with a GPUI fallback (2026-09-30)

**Status:** Accepted by user decision on 2026-09-30, **pending confirmation by the M0 spikes** (S1–S6). A failed spike is decided separately, as below.

**Status update, 2026-09-30:** the headless M0 spikes support the stack. S1, S3, S5 and the startup part of S6 pass or have mitigations the user approved on 2026-09-30 ([ADR-014](#adr-014--syntax-highlighting-size-policy-2026-09-30), [ADR-015](#adr-015--exact-tab-stops-in-pango-units-2026-09-30), [ADR-016](#adr-016--column-mode-editing-and-undo-model-2026-09-30), and the amendments to [ADR-003](#adr-003-amendment--bulk-edits-by-edit-count-2026-09-30) and [ADR-004](#adr-004-amendment--reload-trigger-for-a-missing-current-directory-2026-09-30)). S2's Replace All failure is addressed by the [ADR-005 amendment](#adr-005-amendment--our-own-replace-engine-2026-09-30). The live Hyprland checks (S4) and the live re-measures of S1, S5 and S6 ran later that day and support the stack too ([S4-live.md](spikes/S4-live.md)). A few live items were not completed; moving them into the milestones that build them is proposed in [PLAN.md](PLAN.md#m0-live-checks-on-hyprland--2026-09-30). M0 closes when the user approves the preview gate. **Closed on 2026-10-01 with GO** (user decision).

**Decision:** Build the UI with plain gtk4-rs (no relm4):
- `gtk4` 0.11.5 with `v4_18`, `libadwaita` 0.9.2 with `v1_9`, `sourceview5` 0.11.2 with `v5_18` (the crate has no `v5_20` flag);
- system libraries already installed: gtk4 4.22.4, libadwaita 1.9.3, gtksourceview5 5.20.0.

Fallback: GPUI + gpui-component. The `domain` and `infrastructure` crates carry over unchanged.

**Reason:**
- The toolkit research scored the options out of 100: GTK4 + libadwaita + GtkSourceView **84**; Qt Widgets + QScintilla 80 (but the UI would be mostly C++, because CXX-Qt does not bind Qt Widgets); GPUI + gpui-component 71.5; CXX-Qt + QML (the Hertz stack) 66.5; Floem, iced/cosmic-text, egui, Slint, Makepad and Xilem 35–52.
- GtkSourceView and GTK provide most of the everyday feature set: the buffer with undo grouping, 180 languages (compiled into the library as GResources), style schemes reloadable at runtime, bracket matching, `SearchContext` on PCRE2 with JIT, the space drawer, the Map, completion, gutters and marks.
- The shell comes with it: AdwTabView/AdwTabBar, GtkPaned, AdwBanner/AdwToast, GMenuModel, GtkShortcutController, GFileMonitor, GApplication single instance over D-Bus, accessibility, text-input-v3, fractional scaling, and portal file dialogs (the default since GTK 4.18; Omarchy already floats them).
- GTK4 has its own Wayland backend, so dragging files from Nautilus works. Every winit-0.30-based toolkit (iced, egui/eframe, Slint's winit backend, Xilem) lacks native Wayland file drag-and-drop.

**Alternatives:** GPUI + gpui-component 0.7.0 has multi-cursor, column selection and folding built in. Against it: the crates.io `gpui` has been frozen at 0.2.2 since 2025-10-22, downstream depends on weekly `gpui-pre` snapshots, `gpui-ce` was yanked on 2026-08-28, it needs a Vulkan-capable GPU, has fractional-scaling and accessibility rough edges, and no native menus or printing. CXX-Qt + QML: QML TextEdit has almost no editor features.

**Consequence:**
- Known weak spots: GtkTextView is slow on very long lines (GTK #229) and huge files; there is no multi-cursor, column selection or folding; GtkSourceView's main maintainer announced very limited involvement (Feb 2026), though it is GNOME core and 5.22 shipped in Sept 2026.
- S1 gates the project. An S1 miss leads to an ADR for a large-file viewer; an S5 miss reopens "column mode in 1.0, or GPUI".
- Only `app` links GTK. No C++ toolchain is needed. Runtime depends: `gtk4 libadwaita gtksourceview5 glib2 pcre2 fontconfig`.

**References:** [sourceview5 features](https://docs.rs/crate/sourceview5/latest/features), [libadwaita crate features](https://docs.rs/crate/libadwaita/latest/features), [GtkSourceView NEWS](https://gitlab.gnome.org/GNOME/gtksourceview/-/raw/master/NEWS), [GTK #229](https://gitlab.gnome.org/GNOME/gtk/-/issues/229), [Hergert, "Mid-life transitions"](https://blogs.gnome.org/chergert/2026/02/06/mid-life-transitions/), [gpui-kit](https://github.com/longbridge/gpui-kit).

## ADR-003 — The buffer is the source of truth (2026-09-30)

**Status:** Accepted — user decision, 2026-09-30, as amended below: the user approved the plan that set this direction (plan §3.2) and then the amendment. Drafted in M0; S1 (snapshot cost), S2 (bulk replace) and S5 (column edits) test it. All three ran on 2026-09-30: S1's snapshot cost is far below the revisit trigger, and S2 and S5 led to the [amendment below](#adr-003-amendment--bulk-edits-by-edit-count-2026-09-30), which the user accepted.

**Decision:** The GtkSourceBuffer owns the text and the undo history. There is no ropey mirror.
- `domain` operations are pure functions over `&str` snapshots that return `Vec<TextEdit>` in character offsets, which match GtkTextIter offsets.
- `apply_edits` applies them in reverse order inside one user action, so each operation is one undo step.
- Above a threshold, a bulk operation replaces the whole range once and remaps marks and bookmarks, instead of making up to 1M edits.
- The undo history is capped.

**Reason:**
- The crates research recommended ropey 1.6.1 with our own undo only for the case where the core owns the buffer. With GtkSourceView, a rope would be a second copy kept in sync through signals: twice the memory and a class of sync bugs.
- Technical critique #11: applying 1M small edits means 1M buffer mutations and 1M undo entries (GtkTextHistory `max_undo_levels` defaults to 0, unlimited), and fires `changed` handlers and search rescans for each one.
- Pure `domain` functions stay toolkit-free, so they carry over to the GPUI fallback and are proptest-friendly.

**Alternatives:** A ropey 1.6.1 mirror (`default-features = false`, `simd` + `cr_lines`) updated from `insert-text`/`delete-range`, for O(1) snapshots.

**Consequence:** Workers receive owned snapshots tagged `(DocId, revision, generation)`; results for an old revision are dropped. Undoing back to the save point clears `modified` (confirmed in `gtktexthistory.c` by the technical critique). **Revisit trigger:** if S1 shows that snapshotting 50 MB costs more than 100 ms, add the ropey mirror.

### ADR-003 amendment — Bulk edits by edit count (2026-09-30)

**Status:** Accepted — user decision, 2026-09-30, on the evidence of spikes S2 and S5. It makes "above a threshold" in the decision concrete, and it corrects the reason's claim about `max_undo_levels`.

**Decision:**
- An operation of more than about **2,000 individual edits** is applied as a **bulk edit**: the affected range is replaced once, inside one user action, and marks, bookmarks and carets are remapped from the domain diff. The threshold is an edit count, not a file size.
- **Correction:** `max-undo-levels` on a new GtkSourceBuffer was measured as **200** in S2, not 0 (unlimited) as technical critique #11 and the reason above assumed. The explicit undo cap is set in M1; its value is decided then.

**Reason:**
- S2: with a view attached, undo costs about 0.1 ms and 8.5–8.7 KiB of RSS per edit. Undoing 1M replacements took 121 s and +8.2 GiB; undoing 49,883 took 4.3 s. A whole-text swap in one user action took 1.82 s at 1M lines and undid in 1.37 s.
- S5: undoing 10k per-line column edits took about 250 ms with a view attached, because GTK's undo emitted 20,000 `mark-set` signals. A range rewrite (one delete, one insert) undid in 4.5 ms, but marks inside the range collapse to its start and undo does not restore them, so bookmarks must be saved and restored by line.
- S2: a single history step held 1M edits, whatever `max-undo-levels` says; the cap limits steps, not edits.

**Consequence:**
- `apply_edits` applies up to about 2,000 edits one by one and larger operations as one bulk edit. Both are one undo step.
- Bookmarks inside a bulk range are saved by line and restored afterwards; undo alone does not restore them (S5).
- The undo cap's value is recorded as a further amendment in M1.
- S1 measured snapshots at 8.1 ms for 10 MB and 27.7 ms for 50 MB, far below the 100 ms revisit trigger, so no ropey mirror is added.

### ADR-003 amendment — The undo cap is 1,000 steps (2026-10-01)

**Status:** Accepted — decided in M1 under the user's standing authorization of 2026-10-01 ([PLAN.md](PLAN.md#m0-closed-and-the-whole-plan-authorized--2026-10-01)). It settles the value the amendment above left to M1.

**Decision:** Every buffer is created with `max-undo-levels` = **1,000** (`stet_domain::view::MAX_UNDO_LEVELS`).

**Reason:**
- The cap counts undo steps, not edits: S2 found one step holding 1M edits whatever the cap says. Typing merges into word-sized steps (checked 2026-10-01 in a self-test: "hello world" inserted one character at a time undid in two steps, " world" then "hello"), and ADR-003 makes every operation one step. So 1,000 steps is about 1,000 typed words or operations back, where GtkSourceBuffer's default of 200 (measured in S2) is about 200 words.
- The cost of undoing one step is bounded by the bulk-edit rule: at most about 2,000 edits, which at S2's 0.1 ms and 8.5–8.7 KiB per edit with a view attached is about 0.2 s and 17 MiB of transient RSS.
- The history's memory stays small in normal use. S2's RSS grew from 516 to 880 MiB while one step held 1M replacements, so a stored edit costs at most about 0.37 KiB (that figure also includes the new text and search state). A thousand typing steps are well under 1 MiB; a thousand maximal 2,000-edit steps would be about 0.7 GiB, which is bounded and far from normal use. A bulk step holds its range's old and new text, so its cost follows the document size, not the cap.
- Unlimited undo would have no bound at all.

**Consequence:** The self-test `assert-undo-levels 1000` checks new buffers. Revisit with a size-aware rule if the M3 large-file or M4 Replace All measurements show the history's memory mattering.

### ADR-003 amendment — Loading and reloading files (2026-10-01)

**Status:** Accepted — M3, under the standing authorization of 2026-10-01. It adopts S1's loading recommendations, which the user's review of 2026-09-30 had left open.

**Decision:**
- **Loading** never uses one `set_text`: a worker reads and decodes the file, a loader thread cuts the text into pieces of about 4 MiB at line ends and streams them, and the editor inserts the first at once and each further one 16 ms later on a timer (an idle is starved by GtkTextView's validation, S1). Undo is off, the language is off the buffer and the view is read-only while it loads; a banner shows the progress. The GTK thread never holds the whole text, and the pieces are freed as they go in.
- **A reload from disk is one undoable step:** a worker compares the buffer's snapshot with the new text and the editor replaces only what lies between their common prefix and suffix (`domain::text::minimal_edit`), in one user action, so Undo brings the previous text back and marks outside the change stay. Large files and text from the long-line path load again instead, without undo; Reinterpret too.
- The snapshot for a save, a convert check or a reload is taken on the GTK thread (S1: 8 ms at 10 MB, 28 ms at 50 MB); everything else runs on workers.

**Consequence:** Measured headless with the release build on 2026-10-01: the first screen of a 100 MB log 165 ms after the open, all of it in after 1.45 s, the longest main-loop block 46.6 ms; 200 MB: 237 ms, 3.0 s and 56 ms ([TESTING.md](TESTING.md)).

### ADR-003 amendment — Text tools on workers, large results in pieces (M5, 2026-10-01)

**Status:** Accepted — M5, under the standing authorization of 2026-10-01. The decision and its bulk-edit amendment stand; this records how the text toolbox applies them.

**Decision:**
- **One path for every text tool** (`app/src/window/toolbox.rs`): take a snapshot of the buffer, run the `domain::ops` operation on it, apply its `Change` in one user action (one undo step), then select what the operation asks for (`Change::selection_after`).
- **Where it runs:** documents up to **512 KiB** of text at once on the GTK thread, so a held Ctrl+Shift+Down never drops a step; larger ones on a worker with a **revision check**: when the buffer changed meanwhile, nothing is applied and a toast says so. While a worker runs, the editor takes no edits and other tools, Replace and Replace All wait (`EditorPage::is_busy`).
- **How it is applied:** up to 2,000 edits edit by edit; above that, as **one bulk edit** that replaces the span from the first edit to the last (`Change::coalesce`, computed on the worker). A bulk replacement or a result whose inserted text is over **1 MiB** goes in **in pieces inside the one user action**: the old range is deleted 2 Mi characters at a time and the new text inserted about 1 MiB at a time, cut after a line break, 16 ms apart, with the view read-only meanwhile. Undo restores the whole operation in one step.
- Nothing is line-anchored yet that a bulk edit would lose (bookmarks arrive in M7); the caret and selection are set from the operation's result.

**Reason** (`toolbox-large.stet-test`, headless under Broadway with frames unpaced, 2026-10-01): in the release build, formatting a 20 MB JSON file (one object per line) went in as 43 pieces and took 1.62 s end to end, with the longest main-loop block 37.0 ms; an integer sort of about 270,000 lines (3 MB) took 0.33 s, blocking the main loop for at most 55.0 ms; trimming both ends of the same lines (551,022 edits, one bulk edit) took 0.19 s. In two debug-build runs they took 3.07–3.15 s, 1.42–1.46 s and 0.39–0.41 s, and the longest blocks were 38.1 ms (format) and 59.6 ms (sort). Before deletions were cut into pieces as well, the debug build's longest blocks were 488.2 ms for the format, 145.4 ms for the sort and 339 ms for the trim. The PRD's "formatting a 20 MB JSON file doesn't block the UI" is the bar.

**Consequence:** Undoing such an operation is one GtkTextBuffer step applied at once, so it blocks like M4's Replace All undo: 626.2 ms for the 20 MB format in the release build (reported by the same script, not held to a bar; a follow-up in [PLAN.md](PLAN.md)). The figures are in [TESTING.md](TESTING.md).

### ADR-003 amendment — The first screen of a load, after M2 (2026-10-01)

**Status:** Accepted — found and fixed in the live check of the M2 merge, under the standing authorization of 2026-10-01.

**Decision:**
- **The first piece is about 64 KiB** (`loader::FIRST_CHUNK_BYTES`), a screen's worth; the rest stay about 4 MiB.
- **After the first piece, a load that has more pieces waits** (at most 1 s) until the window is shown, then lets idle work run once, before it streams the rest.
- **The first window appears once the current tab shows text** (`EditorPage::shows_text`): it is loaded, or the first piece of a load in pieces is in. M2's first version waited for the whole load or its 1 s limit.

**Reason:** GtkTextView validates the height of every inserted line in idles that run ahead of GLib's default idle priority, so default-priority idles starve until it is done (S1): 44 s for the 1M-line, 94 MiB fixture on this machine, at 100% of the main thread. GTK registers a new window for accessibility in such an idle. Before M2 the window appeared before any text went in, so it registered at once. M2 showed it only after the load (or after 1 s), when 4 MiB of lines were waiting for validation: in the live session the 10 MB fixture's window registered after 5.3 s, and the 94 MiB one only after 44 s, invisible to screen readers and to the live acceptance script until then.

**Consequence:** Live on 2026-10-01 (release build): both register within 50 ms of their window; the 94 MiB fixture's first screen is painted 706 ms after the process starts (1,722 ms before the fix). Validation still keeps the main thread busy for about 44 s after a 1M-line file opens; typing and painting stay responsive (blocks under 60 ms), but other idle work waits until then. The loader paces itself with timers and the focus fixes use `HIGH_IDLE`, so neither waits for validation.

### ADR-003 amendment — Large column edits keep the view (M6, 2026-10-01)

**Status:** Accepted — M6, under the standing authorization of 2026-10-01. It refines how the bulk-edit rule above applies a column operation.

**Decision:**
- A column operation of more than `BULK_THRESHOLD` (2,000) edits is one user action, so one undo step, made of: the edits on the lines from one screen above the view to two screens below it, applied one by one; and the edits above and below that band, each applied as **one replacement** of its line range (`app/src/column/edit.rs`, `apply_bulk`). An operation of up to 2,000 edits goes in edit by edit.
- Source marks (M7's bookmarks) on replaced lines are made again on the same lines afterwards (`BulkEdit::marks_to_restore`); a column edit never adds or removes lines inside its range. Undo and redo collapse them again (S5), which M7 has to restore.

**Reason:**
- One replacement of the whole range made the view jump when it was inside the range. The replaced lines are new to GtkTextView, which knows their heights only after measuring them again, and GtkTextView's own mark for the top of the view collapses to the start of the replacement. Putting the scroll adjustment back, splitting the replacement at the top line, and scrolling back to a mark afterwards all failed in the headless check; lines edited one by one keep their heights and the mark, and `column-large.stet-test` checks that line 5,000 stays on screen.
- The band is at most a few screens (69 to 103 lines in the headless window), so undo stays far from S5's 250 ms for 10,000 single edits.

**Consequence:** Measured headless (Broadway) with the release build on 2026-10-01, 10,000 lines of S5's shapes, the view at line 5,000 (`perf-column.stet-test`, three runs): inserting at column 0 on every line 14.1–15.2 ms from snapshot to caret, the main loop blocked at most 17.4 ms; undo 24.7–25.8 ms and redo 26.2–27.7 ms, the main loop blocked at most 33.1 ms over both while GTK measured the lines again (idle again after about 0.45 s). An earlier debug build that replaced the whole range at once undid in 4.8 ms and redid in 5.6 ms, but moved the view. Details in [TESTING.md](TESTING.md#2026-10-01--m6-column-mode-ci-self-tests-and-the-10000-line-timings).

### ADR-003 amendment — Bookmarks and marks across bulk edits, Undo and Redo (M7, 2026-10-01)

**Status:** Accepted — M7, under the standing authorization of 2026-10-01. It makes "bookmarks inside a bulk range are saved by line and restored" concrete, and adds what the bulk-edit amendment left open: Undo and Redo.

**Decision:**
- **Bookmarks are GtkSourceView source marks** (category `stet-bookmark`) at line starts, so typing moves them with the text and the gutter draws them. Every edit Stet makes itself goes through `DocMarks::track` (`app/src/marks.rs`): Replace All (in the document, in open documents, and Replace in Files' open documents), the text tools (also when a large result goes in pieces), a reload from disk, the bookmarked-line operations and large column edits. Afterwards the bookmarks are what the domain says (`Bookmarks::remap`, or the operation's own result for the bookmarked-line operations), set by line.
- **Undo and Redo.** GTK replays a step as the replacements it was made of, which collapse the marks inside them again. When a tracked step involved bookmarks or marks, it is kept in a `BookmarkHistory` (`domain::marks::history`) with its **footprint**: where its first replacement started and how many characters it deleted and inserted in all. While GTK replays an undo or a redo (`undo` and `redo` signals around the replay), the replacements it makes are added up; when they match the last step kept (inverted for an undo) and the text has the step's length before (or after) it, the step's bookmarks on its lines come back and those GTK moved outside them stay. Steps go with the edits GTK forgets (a new edit drops the ones that could be redone; at most 1,000 are kept, as GTK keeps 1,000 undo steps). A document that loads again without undo forgets them.
- **Marks** (Mark and the style tokens) are ranges in a `domain::marks::Marks` model, moved through every edit (typing included) and shown with text tags around what the views show. A tracked step keeps the marks from before and after it too, so Undo and Redo put them back instead of dropping those inside the replaced range.
- **One undo step.** A bookmarked-line operation (Cut, Remove, Remove Non-Bookmarked Lines, Paste to Bookmarked Lines) is one user action: edit by edit up to 2,000 edits, above that one bulk replacement. Its edit is computed on a snapshot, on a worker above 512 KiB, with a revision check, as the text tools do.
- **Setting many bookmarks.** A replacement collapses the marks inside it onto one place, and GtkTextBuffer keeps a line's marks in a list that removing a mark walks from its head: GtkSourceView's `remove_source_marks` took 1.06 s for 10,562 collapsed bookmarks (release build). Stet removes each place's marks in the order of that list instead: 0.14 s.

**Reason:** S5 showed that Undo doesn't restore marks inside a replaced range. Recognising Stet's own steps by what GTK replays needs no hook into GtkTextHistory, which has no public API to tag steps, and an undo of anyone else's step simply doesn't match.

**Consequence:** The M7 exit criterion holds headless: marking a regex with Bookmark line and removing the other lines is one undo step that brings the lines and the bookmarks back, on 6 lines and on 105,613 lines (10,562 bookmarks; the line operation's edit on a worker, applied as one replacement; the main loop blocked at most 253.6 ms in the release build and 286.5 ms in the debug build; [TESTING.md](TESTING.md#2026-10-01--m7-marking-split-view-and-compare)). A step whose replay can't be told apart from another edit of the same footprint at the same place would get its bookmarks; that needs an edit GTK replays exactly like Stet's.

## ADR-004 — Follow the live Omarchy theme (2026-09-30)

**Status:** Accepted — user decision, 2026-09-30. Spike S3 validated the mechanism headless on 2026-09-30; the [amendment below](#adr-004-amendment--reload-trigger-for-a-missing-current-directory-2026-09-30) adds one reload trigger.

**Decision:**
- **Palette.** Read `~/.local/state/omarchy/current/theme/colors.toml`. When `/usr/share/omarchy/bin/omarchy-theme-color` exists, run `omarchy-theme-color --file <path> --all` and use its fully resolved, tab-separated palette (about 8 ms on this machine). A Rust port of the fallback cascade is used only outside Omarchy. Stet supplies its own `accent` fallback.
- **Override.** An optional colour-only `current/theme/stet.toml` wins over the generated roles. A `stet.toml.tpl.sample` is shipped for users who want to put a template in `~/.config/omarchy/themed/`.
- **Reload.** A GFileMonitor watches the directory `~/.local/state/omarchy/current/` with `WATCH_MOVES`. It triggers on `theme.name` CHANGES_DONE_HINT, or on RENAMED where the new name is `theme`; events on `next-theme` are ignored. Debounce 150 ms. If the watch can't be set up, poll every 2 s.
- **Apply.** Regenerate `$XDG_CACHE_HOME/stet/styles/stet-omarchy-<hash>.xml` (a unique scheme id avoids stale cached schemes); call `append_search_path` once, then `force_rescan`, then `set_style_scheme` on every buffer; then swap the CSS provider that overrides libadwaita's CSS variables (with `--window-radius: 0`). AdwStyleManager is set to FORCE_DARK or FORCE_LIGHT from `mode`.
- Smart highlight, search matches and bracket match are named custom styles in the scheme and are re-applied after every swap. Mark and style tokens are static GtkTextTags, recoloured on theme change.
- **Font.** `fc-match monospace -f '%{family[0]}'`, re-resolved when `~/.config/fontconfig/` changes, applied through CSS; 11 pt; zoom per session.
- **Outside Omarchy:** follow the portal's dark/light preference with two built-in palettes.

**Reason:**
- Omarchy's own GUI apps (omawrite 0.5.0, omacalc 0.2.2, omacut 0.4.0) read `colors.toml` directly and watch `current/`. GTK apps otherwise get only Adwaita or Adwaita-dark from `omarchy-theme-set-gnome`; there is no `gtk.css`.
- Dark mode and theming is among the most-voted requests the research found (36 upvotes).
- `omarchy-theme-set` builds `current/next-theme`, then runs `rm -rf current/theme; mv next-theme theme` and writes `theme.name`. Technical critique #7: with `WATCH_MOVES` that swap is a RENAMED event, not MOVED_IN or CREATED, so the draft's trigger would never fire. `theme.name` is rewritten right after the swap by both `omarchy-theme-set` and `omarchy-theme-refresh`.
- Technical critique #8: the draft's Rust cascade missed fallbacks that `omarchy-theme-color` applies (the `light.mode` file; `selection` → `selection_background` → `color8` → `color0` → `background`; `lighter_background` → `color0` → `background`; bright/light/dark foreground → `color15`/`color7`/`color8` → `foreground`; ANSI-only themes; the `purple` alias; `cursor` always `bright_foreground`; no `accent` fallback at all). Calling the script avoids drift.
- Shipping only an Omarchy `.tpl` was rejected: a package can't install into `~/.config/omarchy/themed/`, `/usr/share/omarchy/default/themed/` belongs to the `omarchy` package, and templates render only on the next theme set.
- Hertz followed Omarchy (Hertz ADR-009) and later moved to a fixed palette after previews (Hertz ADR-010). For Stet the user explicitly chose to follow the theme.

**Consequence:** S3 must recolour the whole UI and switch live across three themes, including catppuccin-latte (light). The scheme XML and CSS for all 22 built-in `colors.toml` files get insta snapshots. When `lighter_background` equals `background`, the current-line colour is clamped with a mix so it stays visible. A missing `current/` directory means the fallback palettes, not an error. Integration details: [INTEGRATIONS.md](INTEGRATIONS.md); token table: [DESIGN.md](DESIGN.md).

**References:** `/usr/share/omarchy/bin/omarchy-theme-set` (lines 12–13, 292–296), `/usr/share/omarchy/bin/omarchy-theme-color`, `/usr/share/omarchy/default/themed/helix.toml.tpl`, [GFileMonitorEvent](https://docs.gtk.org/gio/enum.FileMonitorEvent.html), [StyleSchemeManager](https://gnome.pages.gitlab.gnome.org/gtksourceview/gtksourceview5/class.StyleSchemeManager.html).

### ADR-004 amendment — Reload trigger for a missing current directory (2026-09-30)

**Status:** Accepted — user decision, 2026-09-30, on the evidence of spike S3. It adds one trigger to the **Reload** bullet above; the rest of ADR-004 stands.

**Decision:** The reload trigger also fires when `current` appears in the parent directory `~/.local/state/omarchy/`: as CREATED, as MOVED_IN, or as RENAMED where the new name is `current` (for example a renamed-away `current` renamed back, as in manual acceptance check 13). This covers a watch that starts before `current/` exists: watch the parent until `current/` appears, then watch `current/`. (Refined during review on 2026-09-30: CREATED alone would miss the rename-back case.) The existing triggers stay, with the 150 ms debounce: `theme.name` CHANGES_DONE_HINT, and RENAMED where the new name is `theme`.

**Reason** (S3, headless under Broadway, 2026-09-30):
- Without the extra trigger, a watch created before `current/` existed missed the first swap. Only a late `CREATED current` arrived, 0.94–1.99 s after the swap, because GLib polls for a missing watched path; the swap's own events were lost.
- With the full rule, every swap, the refresh and the first write of `theme.name` gave exactly one reload, 150 ms after the last event. A burst of three swaps gave one reload with the final theme.
- All 22 built-in and 6 user themes loaded with 0 CSS errors. A theme switch costs about 15 ms (14.6 ms median, from resolving the palette to the GTK work of the first frame).

**Consequence:** M1 implements the full trigger. S3 measured the variant with the monitor on the missing `current/` path itself; the parent-directory watch decided here is not measured yet, so the M1 theme self-test covers the case where `current/` appears after start.

### ADR-004 amendment — As built in M1 (2026-10-01)

**Status:** Accepted — M1 implementation, under the standing authorization of 2026-10-01. It records details the decision left open; the rule itself stands.

**Decision:**
- **Watch.** `app/src/monitor.rs` watches the parent `~/.local/state/omarchy/` all the time, and `current/` while it exists, both with `WATCH_MOVES`. In `current/` it triggers on `theme.name` CHANGES_DONE_HINT and on RENAMED to `theme`. In the parent it triggers when `current` appears (CREATED, MOVED_IN, RENAMED to it) **and when it disappears** (DELETED, MOVED_OUT, RENAMED away), and it starts or cancels the `current/` watch to match. A missing `current/` means the built-in palette, so its disappearing reloads to that palette at once instead of keeping a stale theme. Debounce 150 ms; polling every 2 s only if no monitor can be created.
- **Off the GTK thread.** `omarchy-theme-color` runs on a worker, and so does writing the scheme file; the GTK thread does `force_rescan`, `set_style_scheme` on every buffer, the CSS provider swap and the colour scheme. Exactly one generated `stet-omarchy-<fingerprint>.xml` is kept in `$XDG_CACHE_HOME/stet/styles/` (older ones are deleted when a new one is written).
- **Colour scheme only on a mode change** (S3 recommendation 4): FORCE_DARK or FORCE_LIGHT for an Omarchy theme; DEFAULT, following the system, for the built-in palettes. The theme source is updated before the colour scheme changes, because that change emits `notify::dark`, which reloads only while a built-in palette is in use (otherwise every switch to a dark theme would reload twice).
- **Font.** `fc-match monospace -f '%{family[0]}'` on a worker, applied as CSS on `.stet-editor` (11 pt plus zoom) and `.stet-mono` (tab strip, status bar, palette); `~/.config/fontconfig/` is watched the same way as `current/` (any change in it, or the directory appearing, re-resolves).
- **The first window waits** up to 500 ms for the first theme and font, so it never appears in libadwaita's default colours.
- **Test hook.** `STET_OMARCHY_STATE_DIR` points Stet at another Omarchy state directory; the self-test harness uses its own (`$TMP/omarchy`), where `theme-set` swaps themes line for line like `omarchy-theme-set`.

**Reason:** The M1 theme self-test (headless, 2026-10-01, [TESTING.md](TESTING.md)) passes with this rule: one reload for `current/` appearing after start, one per swap, one for a refresh, one for a burst of three swaps (the last theme wins), and a switch to the built-in palette and back when `current` is renamed away and back.

**Consequence:** The per-theme `stet.toml` override is not built yet; it stays in the decision above for a later milestone ([PLAN.md](PLAN.md) follow-ups).

### ADR-004 amendment — Role colours a theme can't tell apart (M9, 2026-10-01)

**Status:** Accepted — M9, under the standing authorization of 2026-10-01, after the live check of M7's compare in the user's theme.

**Decision:** Compare's four kinds (added, removed, changed, moved: the theme's green, red, blue and magenta) and the five token styles (cyan, orange, yellow, magenta, green) keep the theme's colours when those can be told apart: their hues spread over at least a quarter of the colour wheel and none is nearly grey (saturation under 0.15). Otherwise every colour of that set is turned to its role's usual hue (`domain::theme::distinct`: added 130°, removed 0°, changed 215°, moved 290°; tokens 185°, 30°, 50°, 300°, 120°), keeping roughly its lightness, with at least 45 % saturation.

**Reason:** The user's theme, `dark-music2` (generated by Aether), gives every colour of its palette the same reddish-brown hue: green #926d67, red #b06e65, blue #a86660. Live, compare's added, removed and changed lines came out as (29,23,21), (35,23,21) and (34,21,20) — the same colour — and the five token styles alike. Following the theme must not cost a feature its meaning. Of the 22 themes Omarchy ships, the rule turns matte-black, vantablack, white, hackerman, lumon, lupine (blues and purples only), solitude (greys), ethereal (a nearly grey green) and last-horizon's tokens; the others keep their own colours, near neighbours such as catppuccin's peach and yellow included.

**Consequence:** In `dark-music2` the three kinds now read (14,37,18), (38,18,18) and (15,25,38) live. Unit tests cover the HSL round trip, a single-hue palette, palettes that keep their colours, and greys; the colour snapshots of the snapshotted themes are unchanged.

## ADR-005 — PCRE2 everywhere (2026-09-30)

**Status:** Accepted — user decision, 2026-09-30, as amended below: the user approved the plan that set this direction (plan §3.2) and then the amendment. S2 measures replace-all and templates; the M4 parity suite confirms the details. S2 ran on 2026-09-30, and the user accepted the [amendment below](#adr-005-amendment--our-own-replace-engine-2026-09-30), which moves replacing to our own engine.

**Decision:**
- In-document search uses GtkSourceSearchContext (PCRE2 with JIT).
- Find in Files uses `grep-pcre2` for Regex mode and `grep-regex` fixed strings for Normal and Extended modes.
- Whole word uses the search context's `at-word-boundaries`.
- Boost-style replacement templates (`$0 $1 ${n} \1 \U \L \E`) are expanded by our own code over the captures.
- Boost-only syntax such as `\<` and `\>` is translated in `domain::search`.
- A parity suite of about 60 patterns runs against the real widget.
- `pcre2` is a runtime dependency, so `pcre2-sys` never falls back to its bundled copy.

**Reason:**
- Many users already know the Boost regex dialect and rely on lookbehind, backreferences and `\R`; Rust's `regex` crate has neither lookaround nor backreferences.
- The crates research suggested `fancy-regex`, but GtkSourceView already uses PCRE2, so that would give search two dialects.
- Technical critique (low): `libgtksourceview-5` links `libpcre2-8` directly and uses its own compile options (only `g_regex_check_replacement` comes from GLib), so parity must be tested against the widget. `\R` is native in PCRE2. The `pcre2` crate 0.2.11 has no substitute API, so template expansion over captures is required.

**Alternatives:** `fancy-regex` 0.19 over `regex` 1.13 everywhere; GLib's GRegex.

**Consequence:** One translation layer in `domain::search` (query, Extended unescape, Boost translation, template). The Find in Files matcher and the widget can differ in compile options, so the parity suite runs in CI. System PCRE2 is 10.48 on this machine.

**References:** [SearchSettings](https://gnome.pages.gitlab.gnome.org/gtksourceview/gtksourceview5/class.SearchSettings.html), [pcre2 crate Regex](https://docs.rs/pcre2/latest/pcre2/bytes/struct.Regex.html).

### ADR-005 amendment — Our own replace engine (2026-09-30)

**Status:** Accepted — user decision, 2026-09-30, on the evidence of spike S2. It narrows the first bullet above: GtkSourceSearchContext finds, counts and highlights, but never replaces.

**Decision:**
- GtkSourceView's `SearchContext` stays for highlighting, counting and Find Next/Previous.
- **Replace, Replace All, Replace All in open documents and Replace in Files use our own PCRE2 matching** (the `pcre2` crate) on a snapshot, with our own expansion of Boost-style templates (`$1`, `${1}`, `$&`, `\1`, `\U…\E`, …). In an open document the result is applied through ADR-003 as one undo step, and as one bulk edit above about 2,000 replacements ([ADR-003 amendment](#adr-003-amendment--bulk-edits-by-edit-count-2026-09-30)).
- A pattern-translation layer in `domain::search` prepares patterns for SearchContext:
  - Boost `\<` and `\>` become `\b(?=\w)` and `\b(?<=\w)`;
  - `X\KY` becomes `(?<=X)Y` when X has a bounded length; otherwise the pattern goes to our own matcher;
  - patterns that can match empty text (for example `^` and `$`) go to our own matcher.
- Our own Extended-mode unescape stays: GtkSourceView's `utils_unescape_search_text` has no `\xHH`.

**Reason** (S2, headless under Broadway, 2026-09-30):
- `SearchContext::replace_all` took 8.0 s on the GTK thread for 1M replacements, against the ≤ 5 s bar. Undoing it with the view attached took 121 s and +8.2 GiB RSS.
- `\K` and zero-length matches do not work through SearchContext's navigation and replace: `forward()` finds nothing and `replace_all` replaces 0, although `\K` is counted and highlighted correctly.
- GtkSourceView's replacement syntax takes `\0`–`\99`, `\g<n>` and `\U \L \E \u \l`, but not `$1`, which is inserted literally. It processes escapes only when the template also contains a group reference, so the Boost-style dialect cannot be mapped onto it mechanically in every case.
- PCRE2 reads Boost's `\<` `\>` as literal `<` and `>`, silently; the translation gave the Boost result.

**Consequence:**
- Every replace path has one template dialect. The M4 parity suite tests replacing through this engine; the widget's known exceptions (`\K`, empty matches, `$n`) are routed here.
- Replace All keeps the existing bar (≤ 5 s over 1M lines, one undo step), measured end to end through this path in M4.
- **Upstream bug.** `sourceview5` 0.11.2's hand-written `SearchContext::replace_all` treats the C function's return value, a `guint` count of replaced matches, as a success flag. `assert_eq!(is_ok == 0, !error.is_null())` panics when nothing is replaced and there is no error, and the count is discarded: the method returns `Result<(), glib::Error>`. The draft upstream issue is in [upstream/sourceview5-replace-all.md](upstream/sourceview5-replace-all.md); filing it is the user's action.
- **Workaround.** Production code does not use `SearchContext::replace_all`, nor `SearchContext::replace`, which has the same shape with `debug_assert_eq!`. Both were verified on 2026-09-30 with a headless repro: `replace_all` panics with no match in debug and release builds; `replace` on a range that is not a match panics in debug builds and returns `Ok(())` without replacing in release builds. If a replace through SearchContext is ever needed, call `sourceview5::ffi::gtk_source_search_context_replace_all` through a small wrapper that returns the count, as S2's `replace_all_counted` does.

### ADR-005 amendment — Find Next through our own async wrapper (2026-10-01)

**Status:** Accepted — M1, under the standing authorization of 2026-10-01. It adds a binding workaround; the decision above stands.

**Decision:** Find Next and Find Previous run `gtk_source_search_context_forward_async` / `backward_async` through `app/src/search.rs`, which honours the "found" return value of the `*_finish` functions. Production code does not use `SearchContext::forward_async`, `backward_async`, `forward_future` or `backward_future` from `sourceview5` 0.11.2. The synchronous `forward` and `backward` are correct but block the GTK thread on large buffers.

**Reason:** The generated async bindings ignore the `gboolean` that `gtk_source_search_context_forward_finish` returns and check only the error, so when nothing matches they return `Ok` with uninitialised iterators. In the M1 find self-test (headless, 2026-10-01), searching for text that does not occur through `forward_future` and selecting the result logged `Gtk-CRITICAL … real_set_mark: assertion '_gtk_text_iter_get_btree (where) == tree' failed` twice and the process aborted (core dumped). With the wrapper the same script passes. It is the same class of defect as `replace_all`; the upstream draft covers both ([upstream/sourceview5-replace-all.md](upstream/sourceview5-replace-all.md)).

**Consequence:** The M4 find work keeps using the wrapper until a fixed `sourceview5` release is adopted.

### ADR-005 amendment — Search as built in M4 (2026-10-01)

**Status:** Accepted — M4, under the standing authorization of 2026-10-01. It records how the decision and its amendments were built, and the choices the build made.

**Decision:**
- **One translation, two engines.** Every query is translated once (`domain::search::translate`) and compiled by our matcher (`infrastructure::search::Matcher`), which also reports pattern errors at the typed character. When `Translated::needs_own_matcher` is false, GtkSourceView's `SearchContext` gets the translated pattern with regex search always on, `case-sensitive` from the translation and `at-word-boundaries` off, and counts, highlights and runs Find Next and Previous (through the async wrapper above). Otherwise (patterns that can match empty text, `\K`, `\G`) our matcher counts and navigates (`find_all`, `find_next`) on a snapshot on a worker, and highlights its non-empty matches around the visible lines with a text tag in the scheme's `search-match` colours. Each tab has its own search settings, so a query reaches a tab only while it is shown and hidden tabs never rescan.
- **Whole word lives in the pattern.** Normal and Extended modes spell Stet's rule as lookarounds (`user` becomes `(?<!\w)user(?!\w)`; an end that is punctuation checks for punctuation instead, and an end that is white space is not restricted), and `at-word-boundaries` stays off: GtkSourceView 5.20 implements it in regex mode as `\b` + pattern + `\b` without a group, which breaks alternations, and its rule differs from Stet's. Whole word does not apply in Regex mode, where its button is greyed out.
- **Templates follow Boost's `format_all` dialect**, in our own code (`domain::search::Template`): `$n` takes every following digit (`$10` is group 10), `\1`–`\9` take one (`\10` is group 1, then `0`), `\g<1>` is not a reference, `$&`, `$+{name}`, `\U \L \E \u \l` (one character for one, so `ß` stays `ß`), conditionals, and a `)` that closes nothing ends the template. Two Stet additions: `\0` is the whole match and `${name}` is the named group when the pattern has one. Normal-mode replacements are literal; Extended-mode ones are literal after the Extended escapes, so `\r\n` inserts a line break.
- **Find in Files' newline convention.** Files on disk are searched with `grep-pcre2` (or `grep-regex` for plain text) and the pattern translated for files (`Target::Files`): a line break in the query matches CRLF, LF or CR. `grep-pcre2` cannot take GtkSourceView's newline convention ANY, so files use ANYCRLF: VT, FF, NEL, LS and PS are not line breaks there, and line numbers count LF, so a CR-only file is one line. **Open documents with unsaved changes are searched from their buffer text** with the in-document matcher (LF only, NUL as U+2400) when the walk reaches their file (`FifRequest::open_documents`); the filters and options decide as for any file, and a document whose file is not on disk is not searched.
- **Filters never re-include files.** `ignore` lets an override whitelist win over its hidden-file and `.gitignore` rules, so with the default filter `*.*` the core searched hidden and ignored files (found by the M4 self-test). Only exclusions are walk overrides now; inclusions are checked file by file (test `filters_never_bring_back_hidden_or_ignored_files`).
- **Replacing.** Replace checks the current match, or else the selection, with `replace_one`; when it is not a match it only finds the next one. Replace All and Replace All in Open Documents run `replace_all` on a snapshot on a worker. Edits are applied on the GTK thread in one user action per document, and as one bulk edit (`as_single_edit`) above 2,000 edits; the caret and selection are mapped through the edits; a document that changed while the worker ran is left alone. Replace All in Open Documents asks first.
- **Scope** (`SearchOptions::scope`). In selection applies to Count and Replace All: it uses the selection, or the multi-line selection the bar opened with, which the incremental search would otherwise clear. Find Next, Find All and Replace All in Open Documents ignore it.

**Reason:** The widget parity run proves the split. On 2026-10-01, headless, all 110 rows of the 122-row parity table that the translation leaves to the widget gave GtkSourceView exactly our match spans and count, the 3 invalid patterns that reach it failed in both, one (a lone surrogate) fails in the translation, and the 8 routed rows are our matcher's ([TESTING.md](TESTING.md#2026-10-01--m4-search-ci-self-tests-widget-parity-and-performance)). Replace All over 1M log lines took 2.30–2.34 s end to end, against the 5 s bar, and is one undo step.

**Consequence:**
- Every replace still goes through our engine; `SearchContext::replace` and `replace_all` stay unused.
- A bulk edit blocks the GTK thread while the buffer changes: 1.90–1.94 s for 1M replacements in a 149 MB document (about 0.39 s deleting, the rest inserting), and 1.53–1.56 s to undo it; everything after it runs in idle callbacks that never stalled the main loop for more than 58 ms in the three runs that measured it. Inserting the result in chunks inside the one user action could shorten the stall; that is a follow-up in PLAN.md, not built.
- Large-file mode turns highlight-all and smart highlighting off. Until M3's document flag exists, a document counts as large from the size policy's 50 MiB, measured in characters (`page_search::is_large`).

### ADR-005 amendment — Search over M3's documents (Wave A, 2026-10-01)

**Status:** Accepted — when M3 and M4 were merged (Wave A), under the standing authorization of 2026-10-01.

**Decision:**
- **Large-file mode is the document's.** Highlight-all, our matcher's highlight tag and smart highlighting follow `EditorPage::large_file_mode()`, which the file pipeline sets from the file's size when it reads it (50 MiB, [ADR-003 amendment](#adr-003-amendment--loading-and-reloading-files-2026-10-01)). M4's stand-in, which counted the buffer's characters (`page_search::is_large`), is gone. The find bar applies the mode as soon as a file's size is known, before its text streams in, so an open bar never highlights every match of a large file while it loads (`search-decoded.stet-test`).
- **Find in Files searches more open documents from their text.** Besides those with unsaved changes, every open document whose file the walk would read differently is searched from its decoded buffer text, as the tab shows it: an encoding other than UTF-8 or UTF-16 with a byte-order mark (the walk searches those as bytes, `find_in_files::reads_as_text`), a decoding error, NUL characters (the walk skips such files as binary) or CR line endings (the walk counts lines by LF).
- **Extended `\r\n` and `\0` in loaded documents** behave as the translation says (ADR-007, ADR-008): `\r\n` matches the LF line breaks of a document loaded from a CRLF file, and `\0` the ␀ that stands for a NUL byte; both are self-tests now (`search-crlf.stet-test`, `search-decoded.stet-test`).

**Consequence:** Starting Find in Files copies the text of each such open document on the GTK thread, as M4 already did for documents with unsaved changes; a large clean document in another encoding now costs that copy too. A closed file in another encoding is still searched as bytes, the core's documented limit.

### ADR-005 amendment — Replace in Files (M8, 2026-10-01)

**Status:** Accepted — M8, under the standing authorization of 2026-10-01. It builds the "Replace in Files" of the [amendment of 2026-09-30](#adr-005-amendment--our-own-replace-engine-2026-09-30) and settles what happens to open documents.

**Decision:**
- **One engine, the document's view of the file.** `infrastructure::replace_in_files` walks with Find in Files' walk, filters and options (subfolders, hidden files, `.gitignore`, `!` exclusions; symbolic links are not followed) on worker threads, and matches each file with Replace All's engine (`Matcher::replace_all`, the Boost-style templates) on its text as the editor would open it: detected and decoded (BOM, NUL as U+2400) and normalized to LF. A query therefore means in a closed file what it means in an open document: Extended `\r\n` matches any line break, `\0` a NUL, `^` and `$` work per line.
- **Only the replacements are written.** The LF positions map back to the file's own text (`domain::replace_files`: `LfText`, `raw_edits`, `apply_raw`); a line break in a replacement becomes the file's dominant line ending (LF on a tie or when it has none, as `EolStats::dominant`); each replacement is encoded on its own, strictly, and every other byte is copied from the file (`plan::plan`). The result must decode to the expected text with the same BOM, and every kept stretch, encoded on its own, must give back its bytes; otherwise nothing is written.
- **Left alone, listed with the reason and the number of matches** (`domain::replace_files::Skip`): binary files (NUL bytes that aren't UTF-16), decodes that met malformed bytes or don't round-trip, replacements the encoding can't represent (the characters are named, as in M3's unmappable dialog), read-only files and file systems, files of 50 MiB or more (not read; "open it to replace in it"), files whose fingerprint changed between reading and writing ([ADR-006](#adr-006-amendment--one-fingerprint-type-2026-10-01)'s `matches`), a replacement that would put a lone CR against an LF in a file with mixed line endings (the two would read back as one line break), and stateful encodings whose other bytes would change (ISO-2022-JP when a replacement sits inside an escaped run). Files without a match are never listed, whatever they are.
- **Writing** goes through M3's `safe_save` (a temporary file, fsync and rename, or in place with a backup in `save-backups`), which keeps symbolic links that point at the file, its mode and its owner. A file is written whole or not at all; Stop ends the walk and the matching, and the files already written are listed.
- **Asks first.** "Replace in Files?" names the pattern, the replacement, which files (the filters), the folder, and whether subfolders, hidden files and what `.gitignore` ignores are included, and says that files on disk are changed in place and that can't be undone. Replace in Files is the destructive response; Cancel is the default. There is no preview.
- **Open documents get the edit, not their files.** The walk hands their files back (`RifEvent::OpenDocument`) and the app replaces in the buffer with the same engine on a snapshot: one user action, so one undo step, applied as one bulk edit above 2,000 edits ([ADR-003 amendment](#adr-003-amendment--bulk-edits-by-edit-count-2026-09-30)), the caret and selection mapped. A restored tab whose text isn't loaded yet (an M2 stub) loads first. **A document without unsaved changes is then saved**, so the file on disk agrees with the rest of the tree, unless saving would change other bytes (mixed line endings, a lossy decode, formatted or display-broken text, a pending encoding or line-ending conversion) or the file changed on disk: then it keeps the edit unsaved and says "not saved". **A document with unsaved changes keeps the edit in its buffer, unsaved**; its file is not touched. Undo takes the edit back in the editor.
- **Results:** the results panel lists each file with its count and replaced lines (at most 1,000 lines per file and 50,000 per run, Find in Files' hit cap; later files with their counts only, and so do files reported after Stop), the open documents' outcome ("in the open document, saved", "…, which has unsaved changes", "…, not saved"), the skipped files with their reasons, and failures, under the heading "Replace “a” with “b” (N replacements in M files of K searched; S skipped)". Next and Previous Search Result step through the replaced lines.

**Reason:** Writing only the replacements is the one way to keep every file's encoding, BOM and line endings byte for byte (the M8 exit criterion) whatever the file holds: re-encoding the whole text would normalize mixed line endings, rewrite lossy decodes and drop bytes the user never saw. Matching the decoded, LF-normalized text keeps one query language for documents and files. Saving clean open documents keeps the tree consistent with what the editor shows; never saving a dirty one keeps Stet from writing edits the user hasn't chosen to save.

**Consequence:** The exit criterion holds on the fixture corpus, checked two ways: the unit test `every_fixture_keeps_its_encoding_bom_and_line_endings` (the first word of each fixture replaced with a line break, against bytes encoded independently) and `rif-fixtures.stet-test` in CI (32 fixture files, one run that inserts a line before every line, against bytes computed without any codec) ([TESTING.md](TESTING.md#2026-10-01--m8-navigation-and-replace-in-files)). A clean open document is saved the way any edit of it is (M3's save path encodes the whole text), so in a stateful encoding such as ISO-2022-JP the escape sequences next to a replacement may move, where a closed file in that case is skipped instead.

## ADR-006 — Plain-file session and backups (2026-09-30)

**Status:** Proposed. Implemented in M2.

**Decision:** The session is plain files under `$XDG_STATE_HOME/stet/`, with directories 0700 and files 0600:
- `session.json` (serde, `schema_version`, `#[serde(default)]`) plus `session.json.prev`;
- one backup file per dirty or untitled document;
- a `lock` file.

Every 7 s the pending documents are snapshotted. The writer writes each backup (temp file, fsync, rename) **then** the manifest, so the manifest never points at a missing backup. Large documents back off: over 16 MB at most every 60 s while idle; above the large-file threshold, prompt on quit instead. Restore builds only the active tab eagerly; a changed file shows a conflict banner; orphaned backups reopen as untitled tabs and are never deleted silently; "Forget unsaved drafts" asks first; `--wait` files are excluded.

**Reason:**
- "Never lose work" is the most-cited feature in the research (HN 37960729, 42793075, 30023422, 43673722; a request with 20 upvotes), with dirty and untitled tabs snapshotted every 7 s by default.
- Rewriting multi-MB blobs in SQLite every few seconds costs more than writing files, and plain files are easier to recover by hand (crates research).
- Backups can hold unsaved secrets, so permissions are strict and contents are never logged.

**Alternatives:** SQLite through `rusqlite`, as Hertz uses.

**Consequence:** The backup path depends on the final name, so ADR-010 must be accepted before M2 (renaming afterwards means migrating users' drafts). ADR-010 was accepted on 2026-09-30. The session model must stay forward-compatible; proptest covers it.

### ADR-006 amendment — One fingerprint type (2026-10-01)

**Status:** Accepted — M3, under the standing authorization of 2026-10-01.

**Decision:** `stet_domain::session::Fingerprint` (`dev`, `ino`, `size`, `mtime_ns`, serialized in `session.json`) is the only fingerprint type. `stet_infrastructure::fs` re-exports it and produces it (`fingerprint(path)`, `fingerprint_of(&Metadata)`, `FileMeta::fingerprint()`, `SaveOutcome::fingerprint`); `session_store::fingerprint` is the same function. Every comparison with a baseline uses `Fingerprint::matches`, which ignores `dev`.

**Reason:** The encodings core and the session core were built in parallel with one struct each, and the infrastructure one compared all four fields with `==`. btrfs subvolumes and device-mapper volumes get their device numbers when they are mounted or activated, so `dev` can change across a reboot: an exact comparison would report every open file as changed on disk after a reboot, and the session would see conflicts that aren't there.

**Consequence:** The baseline a load or save records is the value M2's session stores. The external-change checks of M3 and the restore conflict check of M2 cannot disagree.

### ADR-006 amendment — As built in M2 (2026-10-01)

**Status:** Accepted — M2, under the standing authorization of 2026-10-01. It records how the decision was built on the merged session core (`infrastructure::session_store`, `domain::session`) and what was settled on the way.

**Decision:**
- **Backups:** every 7 s (`stet_domain::session::BACKUP_INTERVAL`, a constant until M5's configuration) the window builds the whole session and commits it with the text of every dirty document, and every untitled document with text, whose buffer revision changed since its last backup. A commit that would change nothing is skipped. The backup id is the tab id. Taking the texts is the only work on the GTK thread; the store writes on its own thread.
- **Large documents** (`BackupState::action`): over 16 Mi characters, a document is backed up at most every 60 s and only after 2 s without edits. In large-file mode (≥ 50 MiB, M3) there is no periodic backup; closing the window asks Save / Discard / Cancel about those documents only, and SIGTERM, SIGHUP and SIGINT write them in the last commit anyway.
- **Quitting** (closing the window with SUPER+W, Quit with Ctrl+Alt+Q or Alt+F4, logout) asks nothing else: the window hides, the last commit is written and flushed (at most 2 s, on a worker), `--wait` command lines get their answers (ADR-011), then the window is destroyed. Ctrl+W on a dirty tab still asks. A window without a session (`--no-session`, or a store that could not be opened) asks as before.
- **Signals:** SIGTERM, SIGHUP and SIGINT (`glib_unix::unix_signal_add_local`) do the same, without any prompt.
- **Restore:** the store is opened on a worker, retrying a locked store for 2 s; still locked means another Stet with another application id uses the same state directory (a development build next to the installed one), and that window keeps no session, with a toast. Then the window size and maximized state, the zoom, the recent files and the four find-bar histories come back, every tab as a **stub** (title, dirty marker, path), and the active tab loads before the window is shown (at most 1 s). Other tabs load when first shown, or when an operation needs their text: Save, Close All, Find All and Replace All in open documents, and Find in Files (for those with unsaved changes). A tab with a backup loads its text with its saved encoding, BOM, line ending, language, caret, selection (its direction kept) and first visible line, and the file as it is now: changed since the backup → M3's disk banner, worded "The file “…” changed on disk since your unsaved changes.", with Keep Mine and Load Disk Version (one undoable step); the old baseline stays until it is settled, so it is still a conflict after a restart. Deleted → M3's deleted banner. A tab without a backup opens its file through the file pipeline, in the encoding picked with Reinterpret if any.
- **Lost changes:** a backup that is missing, or a dirty tab without one (large-file mode at a crash), opens from its file, or empty, with the banner "The unsaved changes to “…” from the last session could not be restored." and a Dismiss button. **Orphans** open as untitled tabs with a toast; the next commit that holds their text, or their tab closing, deletes them.
- **Forget Unsaved Drafts** (File menu and palette) confirms first, closes the untitled tabs, reloads files with unsaved changes from disk (one undo step, as M3's reload), commits, then calls `forget_drafts()` on a worker. `--wait` tabs are left alone.
- **Recent files** live in `session.json`. M1's `recent.json` is read once, when the session has no list, and deleted once a commit holding that list is on disk; `infrastructure::recent` stays for that migration and is no longer written.
- **Model extensions**, all `#[serde(default)]` and covered by the round-trip, unknown-field and missing-field proptests: `WindowState::zoom`, `TabRecord::chosen_encoding` (an encoding-table id), `TabRecord::lossy`, `Session::directory_history` and `Session::filter_history` (M4's follow-up). The selection keeps its direction through `caret` and `selection` (`TabRecord::anchor_and_caret`), so no field was needed for it.
- **Not kept:** the undo history, the formatted or display-break origin of long-line text, the mixed-line-ending warning, the NUL placeholder mapping of a file that also had ␀ characters, the word-wrap and whitespace toggles (window settings; M5's configuration), and pinned tabs (M8).

**Reason:** Lazy stubs keep a large session's start to the active tab's cost; loading stubs before operations over all open documents keeps their results complete and makes Save and Close All write the restored text, not an empty buffer. Reusing M3's disk banner for the restore conflict keeps one banner and one set of actions for "the file and the buffer differ", and its persisted baseline needs no new field. A second process with another application id can only meet a locked store briefly (a quitting instance) or for good (a development build next to the installed one); running without a session is safer than waiting forever or writing another instance's session.

**Consequence:** Measured headless on 2026-10-01 ([TESTING.md](TESTING.md#2026-10-01--m2-sessions-backups-restore-and---wait)): 43 dirty and untitled tabs back after SIGKILL with only the typing after the last backup lost (the backup landed 6.6–7.0 s after the edits), after quitting and after SIGTERM exactly as they were; a 50-tab session's window painted its first frame 157.5–166.9 ms after the process started (release build, three runs; 168.2 ms in the debug build); taking a dirty document's text for a backup cost the GTK thread 0.5 ms at 1 MB, 8 ms at 10 MB and 43 ms at 50 MB (release build). The live checks are in [PLAN.md](PLAN.md#m2-built-and-checked-headless-live-checks-pending--2026-10-01).

### ADR-006 amendment — The backup settings (merge of M2 and M5, 2026-10-01)

**Status:** Accepted — at the merge of M2 and M5, under the standing authorization of 2026-10-01. It fills in the plan's M2 outcome "settings to turn backups on or off and set the interval", which M2 left as a constant until M5's `config.toml` ([ADR-017](#adr-017--settings-in-configtoml-and-keystoml-2026-10-01)).

**Decision:**
- `backup_interval` (seconds, 7 by default) sets the backup timer; a change in `config.toml` restarts it at once.
- `backups = false` means **unsaved text is never written anywhere**: no periodic backups, none in the last commit before quitting, and the backups there are deleted at once (the next commit). The session itself still keeps the tabs, the files, the carets and the window. Restored tabs that have not loaded yet keep their backups: they are the only copy of their text.
- With backups off, **closing the window asks** about every document with unsaved changes (Save All, Discard All, Cancel), as it does for documents in large-file mode. A signal (logout, `systemctl --user stop`) cannot ask: the unsaved changes are lost, and the log says how many documents had them. The commented `config.toml` that Open Settings creates says so.

**Reason:** Turning backups off is the choice of someone who doesn't want drafts on disk (secrets in a scratch buffer, a shared machine). Writing them at quit anyway would defeat it, so Stet asks instead.

**Consequence:** `session-settings.stet-test` (in CI) checks the 2 s interval, backups off deleting the backup and writing none, the quit question with Cancel, and backups on again. A new folder for `config.toml` is noticed within GLib's 4 s check for missing files; changes to an existing file within a fraction of a second.

### ADR-006 amendment — Pinned tabs and first-line names (M8, 2026-10-01)

**Status:** Accepted — M8, under the standing authorization of 2026-10-01.

**Decision:**
- **Pinned tabs are kept.** `TabRecord::pinned`, in the model since the session core (`serde(default)`), is now written and restored: a pinned tab comes back pinned, at the start of the strip, a stub included (its record follows the tab's state). This settles M2's "not kept: pinned tabs (M8)".
- **Untitled tabs keep their first-line name.** `TabRecord::display_name` of an untitled document is its first-line name (`domain::untitled::first_line_name`), so a restored stub shows it before its text loads; the untitled number is still kept and continues, and a document emptied of text is "Untitled N" again.

**Consequence:** No schema change; sessions written before M8 restore with no pinned tabs and their untitled tabs named by number until their text loads.

### ADR-006 amendment — Split view, clones and bookmarks (M7, 2026-10-01)

**Status:** Accepted — M7, under the standing authorization of 2026-10-01.

**Decision:**
- **Each tab records its view** (`TabRecord::view`: 0 the main view, 1 the second). A document's first tab (the main view's first) is its record, as before; a clone in the other view is a record of its own (`TabRecord::clone_of`, the document record's id, and its own id) that keeps only its place in its view: caret, selection, first visible line and pinned. Clones of untitled documents go with them in Forget Unsaved Drafts; clones of files stay.
- **The window:** `Session::other_tab` (the tab in front of the other view, when the window is split) and `WindowState::split_position` (the divider in logical pixels; 0 is the middle).
- **Bookmarks** are kept (`TabRecord::bookmarks`, zero-based lines) and come back when the document's text is in; they follow the text as it is then, so a file that changed while Stet was closed keeps them by number. **Not kept:** marks and style tokens, the comparison and the texts it opened (they stay out of the session), and the synchronised-scrolling toggles.

**Consequence:** No schema change: every field is `serde(default)` and lenient, covered by the round-trip, unknown-field and missing-field proptests; `repair` drops an `other_tab` out of range. A session written before M7 restores as one view (`split-session.stet-test` restores one written by hand). What an older build makes of an M7 session with clones was not checked.

### ADR-006 amendment — Names for untitled tabs (1.2, 2026-10-01)

**Status:** Accepted — with the user's decision of 2026-10-01 ([ADR-013 amendment](#adr-013-amendment--names-for-untitled-tabs-12-2026-10-01)).

**Decision:** A tab record has `custom_name`, the name the user gave an untitled document with Rename…; it is written only when there is one, and only for untitled tabs (a clone's record leaves it to its document's record). A restored tab whose text isn't loaded yet keeps it, and a rename of such a tab is written at the next commit. `display_name` holds the name shown, so it is the custom name too.

**Reason:** The name must survive a restart like the text it names. Leaving the field out when it is empty keeps every other session byte for byte as before.

**Consequence:** Older Stet versions ignore the field and show `display_name` until the text is loaded, then the first-line name. `schema_version` stays 1: the field is optional, as every field added since M2.

## ADR-007 — Own encoding pipeline (2026-09-30)

**Status:** Proposed. Implemented in M3.

**Decision:** Stet decodes and encodes files itself instead of using GtkSourceFileLoader/FileSaver.
1. Detect: BOM, then valid UTF-8, then a UTF-16 NUL-pattern heuristic (otherwise the file is binary), then `chardetng`. Decode with `encoding_rs`.
2. Save: encode strictly (`encode_from_utf8_without_replacement`), with a dialog if a character can't be represented. UTF-16 is encoded by hand.
3. At load, re-encode and compare with the original bytes; a mismatch sets `lossy_roundtrip` and shows a banner.
4. Map U+0000 to a placeholder (␀, U+2400) on load and back on save, with a per-document check that the placeholder wasn't already in the file. Extended-mode `\0` maps to the placeholder. Every chunk insert is checked.
5. Binary files open read-only.

**Reason:**
- GtkSourceFileLoader only tries candidate encodings in order (`UTF-8`, `CURRENT`, `ISO-8859-15`, `UTF-16` by default), so Windows-1252, Shift_JIS and GBK files are misread (toolkit research).
- `encoding_rs`'s `encode` silently writes HTML numeric references such as `&#20013;` for unrepresentable characters, and it has no UTF-16 encoder (crates research).
- Technical critique #2: `gtk_text_buffer_emit_insert` rejects text that fails `g_utf8_validate`, which any embedded NUL does, and silently drops the **whole** insert. The next save would lose data.
- Technical critique #14: WHATWG Shift_JIS and GB18030 are not byte-bijective, and unpaired UTF-16 surrogates decode to U+FFFD, so "every fixture round-trips byte-for-byte" can't pass. The criterion became "round-trips or is flagged lossy on open".

**Consequence:** Reinterpret (reload bytes with another encoding) and Convert (change the encoding used on the next save) are separate commands. M3 carries the fixture corpus. The full character-sets menu and OEM code pages are 1.x.

**References:** [encoding_rs](https://docs.rs/encoding_rs/), [chardetng EncodingDetector](https://docs.rs/chardetng/latest/chardetng/struct.EncodingDetector.html), [gtktextbuffer.c](https://raw.githubusercontent.com/GNOME/gtk/main/gtk/gtktextbuffer.c).

### ADR-007 amendment — As built in M3 (2026-10-01)

**Status:** Accepted — M3, under the standing authorization of 2026-10-01. The decision above stands; this records how the app uses it.

**Decision:**
- **Every file goes through the pipeline.** `infrastructure::fs::open_document`/`open_document_as` on a worker, `save_document` on a worker with a snapshot; M1's UTF-8-only `text_file` is gone. The tab keeps the encoding, BOM, line ending, detection source and the decode flags (`had_errors`, `lossy_roundtrip`, `binary`, NUL count, placeholder conflict) for the status bar, the banners and the next save.
- **Status bar:** the short label of the format the next save writes: `UTF-8`, `UTF-8 BOM`, `UTF-16 LE BOM`, `Windows-1252`, `Shift_JIS` (`infrastructure::encoding::status_label`). It opens the **encoding picker**: every `ENCODINGS` entry by group, filterable, in Reinterpret As or Convert To mode. It is a popover of its own rather than a menu, because a popover menu names its submenu pages by their labels, and the same groups under Reinterpret and Convert collided (GTK warned 17 times and the second set of pages was unreachable).
- **Encoding menu**, in this order: Encode in UTF-8, UTF-8-BOM, UTF-16 BE BOM, UTF-16 LE BOM (Reinterpret), Character Sets › one submenu per group (Reinterpret), then Convert to ANSI (Windows-1252), UTF-8, UTF-8-BOM, UTF-16 BE BOM, UTF-16 LE BOM. Each is a registry action, so the palette lists them; the character sets and the picker go through the `reinterpret` and `convert-encoding` actions with an encoding id (`EncodingEntry::id`, the WHATWG name with `+bom`). "Encoding…" in the palette opens the picker.
- **Reinterpret** reads the file's bytes again in the chosen encoding, after asking when that discards unsaved changes; the choice sticks for later reloads. It needs a file: untitled documents can only convert.
- **Convert** checks the snapshot with `check_encodable` on a worker. When characters are missing, the **unmappable dialog** lists the first 20 with their line, column and code point, each with a Go To button (`buffer.iter_at_offset`), says how many there are, and offers "Convert to UTF-8" (or "Save as UTF-8" when a save met them) and Cancel. A conversion marks the document modified; the format change counts as unsaved even when Undo returns the text to the save point.
- **Saving asks first** when the bytes written would differ from what was read in ways the user may not expect: a lossy decode ("Save Anyway"), text opened formatted by the long-line path, mixed line endings. Save As to another file doesn't ask.
- **NUL and binary:** NUL shows as ␀ and is written back as NUL. A file detected as binary opens read-only; "Edit Anyway" is offered when it round-trips (no lossy decode, no mixed line endings). When the file already had ␀ characters and also has NUL bytes, a banner says the NULs can't be written back; with ␀ characters alone nothing is shown, since they round-trip.

**Consequence:** The fixture corpus of the infrastructure tests is also a self-test (`tests/selftest/encodings.stet-test`): every fixture opens with the right encoding and line ending and saves byte for byte, or is flagged lossy when it opens.

## ADR-008 — LF-normalized buffer (2026-09-30)

**Status:** Proposed. Implemented in M3.

**Decision:** The buffer always holds LF line endings. The loader counts CRLF, LF and CR, records the majority as the document's line ending, and normalizes to LF; the saver restores the recorded ending. A file with mixed endings shows a "Mixed" banner. Converting line endings changes the document property and is one undoable step.

**Reason:**
- One representation keeps `TextEdit` offsets, line operations and search uniform: `\R` and Extended-mode `\r\n` translate to `\n` in the buffer, which the M4 parity suite checks ("Extended `\r\n` in a CRLF document").
- It matches the model users expect: the line ending is a document property shown in the status bar and converted as a whole (SO 676k "Find CRLF", 219k "EOL conversion").

**Alternatives:** Store text verbatim so mixed-EOL files round-trip exactly (the crates research's suggestion for a Rust-owned rope).

**Consequence:** A mixed-EOL file cannot be saved byte-identically after editing; the Mixed banner offers to normalize, and exact mixed preservation is on the Later list if anyone needs it. CR-only and CRLF fixtures are in M3, and proptest covers EOL round-trips.

### ADR-008 amendment — As built in M3, and line endings outside the undo history (2026-10-01)

**Status:** Accepted — M3, under the standing authorization of 2026-10-01.

**Decision:**
- The loader normalizes to LF in the decoding pass and keeps the counts; the status bar shows `LF`, `CRLF`, `CR`, or `Mixed` until the mixed endings are normalized or saved.
- **Mixed banner:** "Mixed line endings (1 LF, 2 CRLF, 1 CR): saving writes CRLF everywhere." with "Normalize to CRLF" (the document becomes modified, and the save asks nothing) and "Keep" (the banner goes; the save still asks, since the endings can't be kept).
- **Converting line endings** (Edit › EOL Conversion and the status bar's menu: Windows (CR LF), Unix (LF), Macintosh (CR)) changes the document's save format and marks it modified. **Deviation:** it is not an undoable step. The buffer holds LF either way, so there is no text change for GtkSourceView's history to record; converting back is the undo. The format change counts as unsaved even when Undo returns the text to the save point.

**Reason:** Putting a format change into the undo history needs our own history entries next to GtkTextBuffer's, a second history for one property. Users convert line endings rarely and on purpose.

## ADR-009 — Classic editor keymap with Omarchy remaps (2026-09-30)

**Status:** Accepted — user decision, 2026-09-30. Spike S4 checks key reachability in the live session.

**Decision:** Stet's default keymap uses familiar shortcuts, with the Omarchy remaps below and overrides in `keys.toml`:
- Ctrl+D duplicate, Ctrl+Q toggle comment, Ctrl+L / Ctrl+Shift+L cut / delete line, Ctrl+Shift+Up/Down move line, Ctrl+J join, Ctrl+H replace, Ctrl+Shift+F find in files, F3, Ctrl+G, Ctrl+B, Ctrl+Tab MRU.
- F1 opens the palette. Alt+F4 quits; Ctrl+Q is **not** quit.

Remaps for Omarchy:
- Ctrl+Space belongs to fcitx5, so completion uses Ctrl+Enter.
- Ctrl+Alt+F-keys switch virtual terminals, so the volatile find keys use Alt+F3.
- fcitx5's Unicode addon grabs Ctrl+Shift+U **and** Ctrl+Alt+Shift+U, so UPPERCASE uses another key (chosen in M5). The user guide documents how to clear the grab in `~/.config/fcitx5/conf/unicode.conf`.

Built-in bindings overridden with an editor-scoped, capture-phase controller:
- GtkSourceView: Ctrl+Shift+X/A (`change-number`), Alt+Shift+arrows (`move-viewport`, needed for column select), Alt+Left/Right (`move-words`), Alt+Up/Down (`move-lines`);
- GtkTextView: Ctrl+/ and Ctrl+\;
- AdwTabView: its Alt+1..9 and Ctrl+Tab shortcuts are disabled so the MRU order works;
- `move-to-matching-bracket` is reused for Ctrl+B.

**Reason:**
- The primary users already have these shortcuts in their muscle memory.
- Omarchy puts almost everything on SUPER, and these shortcuts use no Super key. `SUPER+C/V/X` send Ctrl+C/V/X to non-terminal windows, so standard clipboard keys must stay.
- Key conflicts are documented: fcitx5's Ctrl+Space (Omarchy #4842); VT switching on Ctrl+Alt+F-keys; technical critique #6 found both UPPERCASE keys in `/usr/lib/fcitx5/libunicode.so` defaults; technical critique #5 listed the GtkSourceView `class_init` bindings.
- Known convention clashes are accepted: Ctrl+Q quits most GTK apps (harmless because the session persists); Ctrl+D and Ctrl+/ mean something else in VS Code, the user's current default editor. A VS Code-style preset is 1.x.

**Consequence:** The capture-phase controller is scoped to the editor so it never fires in text entries. Accelerators come from the `ActionId` registry. The exact replacement key for UPPERCASE is settled in M5 and recorded as an amendment.

**Live check, 2026-09-30** ([S4-live.md](spikes/S4-live.md)):
- **fcitx5's grabs are confirmed.** Ctrl+Shift+U opens fcitx5's Unicode entry (`U+` preedit), and Ctrl+Alt+Shift+U never reaches the app.
- Ctrl+Space reached the app on this machine, where fcitx5 has a single input method. Completion stays on Ctrl+Enter, which also reached it.
- Ctrl+D, Ctrl+Q, Ctrl+Shift+Up, Alt+Shift+Down and Ctrl+Tab reach the app, and no planned shortcut collides with a Hyprland bind.
- **Answered on 2026-10-01:** the user's F-row sends media keys by default, which is why F1, F11 and Alt+F3 produced no key event. **Every F-key action therefore gets an F-key-free alternative.** The keys are chosen in M1 and recorded as an amendment here.
- The user's layout is Norwegian, so M1's keymap audit covers shifted and AltGr punctuation (`=`, `/`, `\`, brackets).

**References:** [gtksourceview.c](https://raw.githubusercontent.com/GNOME/gtksourceview/master/gtksourceview/gtksourceview.c), [Omarchy #4842](https://github.com/basecamp/omarchy/issues/4842), [Fcitx Unicode](https://fcitx-im.org/wiki/Unicode).

### ADR-009 amendment — F-key-free keys and the Norwegian layout (2026-10-01)

**Status:** Accepted — M1, under the user's standing authorization of 2026-10-01, after the user answered that the F-row sends media keys unless Fn is held.

**Decision:**
- **Every action on an F-key also has a key without one.** The unit test `no_action_is_reachable_only_through_an_f_key` in `domain/src/actions.rs` fails otherwise, for every action added later too. The F-key-free key is listed first, so the hamburger menu (which shows one key) shows it; the palette shows every key.

  | Action | F-key | F-key-free keys |
  | --- | --- | --- |
  | Command palette | F1 | Ctrl+Shift+P |
  | Find Next, Find Previous | F3, Shift+F3 | Alt+Down, Alt+Up; Enter and Shift+Enter in the find bar |
  | Full Screen | F11 | Alt+Enter (and Hyprland's SUPER+F) |
  | Quit | Alt+F4 | Ctrl+Alt+Q |

- **Why these keys.** Ctrl+G / Ctrl+Shift+G, the GNOME and browser convention for find next and previous, is out because Ctrl+G is Go to Line. Alt+Up/Down is free apart from GtkSourceView's own Alt+Up/Down (move lines), one of the built-ins this ADR overrides anyway. Alt+Enter is the Windows console convention for full screen. Ctrl+Q and Ctrl+Shift+Q are the comment keys (M5), so Quit gets Ctrl+Alt+Q. Ctrl+Shift+P is VS Code's palette key.
- **Checked against** fcitx5 5.1.22's default hotkeys found in the installed libraries (Ctrl+Space, Ctrl+Alt+P "toggle preedit", Ctrl+Shift+U, Ctrl+Alt+Shift+U, and the quick-phrase keys on SUPER) and Omarchy's non-SUPER Hyprland binds (S4). The unit test `keys_avoid_known_grabs` keeps those, Ctrl+Alt+F-keys and SUPER out of the registry.
- **Norwegian layout.** In xkeyboard-config's `no(basic)`, `+`, `-`, `\`, `,`, `.` and `'` are unshifted; `=` (Shift+0), `/` (Shift+7), `;` and `:` are shifted; `[`, `]`, `{`, `}`, `@` and `$` need AltGr. **Correction to S4 finding 6:** `\` is unshifted on `no`. The rule: no key may need AltGr on `us` or `no`, and every action on a punctuation key has one key that is unshifted on each layout (unit test `punctuation_keys_work_on_the_us_and_norwegian_layouts`). Zoom In is Ctrl++ (unshifted on `no`), Ctrl+= (unshifted on `us`) and Ctrl+Num+; Zoom Out Ctrl+- and Ctrl+Num-; Reset Zoom Ctrl+0 and Ctrl+Num/.
- **Other M1 keys.** Next and Previous Tab are Ctrl+PgDn and Ctrl+PgUp plus Ctrl+Tab and Ctrl+Shift+Tab, as plain next and previous until the MRU switcher (M8); Move Tab Forward and Backward are Ctrl+Shift+PgDn and PgUp. AdwTabView's own shortcuts are switched off, because its Ctrl+Home and Ctrl+End would take the document start and end away from the editor; the registry owns these keys.
- **Editing keys stay GtkTextView's own bindings:** Undo Ctrl+Z; Redo Ctrl+Y and Ctrl+Shift+Z; Cut Ctrl+X and Shift+Del; Copy Ctrl+C and Ctrl+Ins; Paste Ctrl+V and Shift+Ins; Delete; Select All Ctrl+A. GTK handles application accelerators in the capture phase, before the focused widget, so installing these would take them from the find entry. The registry lists them as widget keys: the menu shows them, the Edit actions act on the editor, and the self-test `assert-widget-keys` checks that GtkTextView binds each one.
- Ctrl+0 is also a candidate for the jump to the next Find Mark style (Search › Jump Down). Reset Zoom takes it now; M7's marking work decides whether that jump needs another key.

**Reason:** Without Fn the user's F-row sends media and brightness keys, which Hyprland consumes, so an F-key-only command is unreachable for them. Users whose F-keys work lose nothing: the F-keys stay.

**Consequence:** The live session still has to confirm that the F-key-free keys reach the app ([PLAN.md](PLAN.md), M1 entry). M5's `keys.toml` builds on the same registry and tests.

### ADR-009 amendment — Search keys (M4, 2026-10-01)

**Status:** Accepted — M4, under the standing authorization of 2026-10-01.

**Decision:**

| Action | Stet keys (menus show the first) |
| --- | --- |
| Replace… | Ctrl+H |
| Find in Files… | Ctrl+Shift+F |
| Search Results Window (focus the panel, or back to the editor) | Ctrl+Alt+R, F7 |
| Next Search Result | Ctrl+Alt+Down, F4 |
| Previous Search Result | Ctrl+Alt+Up, Shift+F4 |

- The find bar's commands (Replace Next, Replace All, Replace All in Open Documents, Count, Find All in Current Document, Find All in Open Documents), Stop Find in Files and the results panel's Copy, Clear and Close are registry actions without keys or menu entries. The palette reaches them.
- Keys handled by the bar itself, not accelerators: Enter and Shift+Enter (Find Next and Previous in the find field, swapped by Backward direction; Replace in the replace field; Find in Files in the files row), Up and Down (the field's history), Escape (close). In the results panel: Enter or double-click (go to the hit), Escape (close the panel), Ctrl+C (copy the selected rows).

**Why these keys.** Ctrl+Alt+Down and Ctrl+Alt+Up are free, are not among Omarchy's non-SUPER binds or fcitx5's hotkeys, and pair with Alt+Down and Alt+Up for Find Next and Previous. The Search Results Window takes Ctrl+Alt+R, R for results. The registry's unit tests (no key twice, no action only on an F-key with the F-key not shown first, grabs avoided, punctuation rules) pass with the new keys.

**Consequence:** The live session has to confirm that Ctrl+Alt+Down, Ctrl+Alt+Up and Ctrl+Alt+R reach the app, and F4, Shift+F4 and F7 with Fn.

### ADR-009 amendment — Text-tool keys (M5, 2026-10-01)

**Status:** Accepted — M5, under the standing authorization of 2026-10-01. It settles the UPPERCASE key this ADR left to M5 and records the M5 keys.

**Decision:**

| Action | Stet keys (menus show the first) |
| --- | --- |
| Duplicate Current Line | Ctrl+D |
| Cut / Copy / Delete Current Line | Ctrl+L / Ctrl+Shift+X / Ctrl+Shift+L |
| Transpose Current Line | Ctrl+T |
| Move Up / Down Current Line | Ctrl+Shift+Up / Down |
| Join Lines | Ctrl+J |
| Insert Blank Line Above / Below Current | Ctrl+Alt+Enter / Ctrl+Alt+Shift+Enter |
| UPPERCASE | Alt+Shift+U |
| lowercase / Proper Case / Sentence case | Ctrl+U / Alt+U / Ctrl+Alt+U |
| Proper Case (blend) / Sentence case (blend) / iNVERT cASE | no key |
| Toggle Single Line Comment / Comment / Uncomment / Block Comment | Ctrl+Q / Ctrl+K / Ctrl+Shift+K / Ctrl+Shift+Q (Block Comment toggles) |
| Go to / Select to Matching Brace | Ctrl+B / Ctrl+Alt+B, through GtkSourceView's `move-to-matching-bracket` (Select also takes the far bracket) |
| Select and Find Next / Previous | Ctrl+Alt+F / Ctrl+Alt+Shift+F, then Ctrl+F3 / Ctrl+Shift+F3 |
| JSON Format / Minify / Validate | Ctrl+Alt+Shift+M / C / J |
| XML Format / Validate | Ctrl+Alt+Shift+B / X |

- **UPPERCASE is Alt+Shift+U.** fcitx5 takes Ctrl+Shift+U and Ctrl+Alt+Shift+U (confirmed live, S4), and Alt+Shift+U keeps the U of every case key; Proper Case (blend), the least used case command, gets no key. A user who frees Ctrl+Shift+U from fcitx5 adds it in `keys.toml` ([INTEGRATIONS.md](INTEGRATIONS.md#6-fcitx5-key-grabs)).
- **The text tools' keys are editor-scoped** (`KeyScope::Editor`): a capture-phase GtkShortcutController on each editor, whose shortcuts come from one list model that the keymap fills, runs them while that editor has the keyboard focus, before GtkSourceView's and GtkTextView's bindings. Other keys stay application accelerators. So Ctrl+D, Ctrl+U or Ctrl+Shift+Up typed in the find bar or the results panel never change the text; the menu, the palette and the editor's context menu run the tools on the current tab whatever has the focus.
- **The built-in bindings this ADR overrides** are in the registry (`OVERRIDDEN_BUILTINS`): Ctrl+Shift+X and Ctrl+Shift+A (`change-number`, and GtkTextView's unselect-all on the latter), Alt+Up/Down and their keypad keys (`move-lines`, a second way to do what Ctrl+Shift+Up/Down does), Alt+Left/Right and keypad (`move-words`; M8's Back and Forward need them), Alt+Shift+Up/Down/PgUp/PgDn/Home/End and keypad (`move-viewport`; column selection, M6), Ctrl+/ and Ctrl+\ (GtkTextView's select-all and unselect-all). The editor's controller takes each one that no text tool uses and does nothing with it; Ctrl+Shift+X is Copy Current Line, and Alt+Up/Down stay Find Previous and Next, whose application accelerators run before the editor's controller. AdwTabView's own shortcuts (Alt+1…9, Ctrl+Tab, …) stay off as in M1. The self-test `assert-editor-keys` checks that every such key is a real GtkSourceView or GtkTextView binding and that the editor takes it.
- **Ctrl+C and Ctrl+X without a selection copy and cut the caret's line**, through the view's `copy-clipboard` and `cut-clipboard` signals, so the Edit menu's Copy and Cut behave the same; with a selection they stay GtkTextView's own.
- **Why these keys.** The keys most users already know, where these commands have them. Select and Find Next and Previous need keys without their F-key (this ADR's first amendment): Ctrl+Alt+F and Ctrl+Alt+Shift+F, because F suggests find. JSON and XML go on Ctrl+Alt+Shift letters, which Omarchy's non-SUPER binds and fcitx5 leave free (fcitx5 takes only Ctrl+Alt+Shift+U there): M, C and J for JSON's Format, Minify and Validate, B and X for XML's Format and Validate. The registry's unit tests (no key twice, F-key alternatives first, grabs avoided, the punctuation rules) pass with every new key; none uses punctuation.

**Consequence:** The live session must confirm that the new keys reach the app on the Norwegian layout (PLAN.md's M5 list). `keys.toml` can change any of them ([ADR-017](#adr-017--settings-in-configtoml-and-keystoml-2026-10-01)).

### ADR-009 amendment — Column-mode keys (M6, 2026-10-01)

**Status:** Accepted — M6, under the standing authorization of 2026-10-01.

**Decision:**

| Action | Stet keys (menus show the first) |
| --- | --- |
| Column Select Left, Right, Up, Down | Alt+Shift+←, →, ↑, ↓, and Alt+Shift with the keypad's arrows |
| Column Select to Line Start, to Line End | Alt+Shift+Home, End, and the keypad's |
| Column Select Page Up, Page Down | Alt+Shift+PgUp, PgDn, and the keypad's |
| Begin/End Select in Column Mode (Edit menu) | Alt+Shift+B |
| Column Editor… (Edit menu) | Alt+C |

- **How Alt+Shift+arrows are taken from GtkSourceView.** They are application accelerators of registry actions (`Keys::app`), like every other registry key. GTK runs the window's application shortcuts (the `gtk-application-shortcuts` controller, capture phase, on the window) before the focused view sees the key, so GtkSourceView's own class bindings never run: `move-viewport` on Alt+Shift+Up, Down, Home, End, PgUp and PgDn (Alt+Shift+Left and Right have no binding in GtkSourceView 5.20). Nothing in GtkSourceView is unbound or overridden, and no editor-scoped controller is involved, which the original decision foresaw for these keys. The self-test `assert-key-route` checks each key's first handler and prints the binding it shadows.
- **What that means elsewhere.** Like every application accelerator, these keys act on the current editor while the focus is in the find bar or the results panel; GtkText has no Alt+Shift+arrow bindings, so nothing is taken from the find field. For M5's `keys.toml`: the column keys are ordinary registry keys and remap like any other. An editor-scoped capture-phase controller (ADR-009's plan for Alt+Up/Down, Alt+Left/Right and Ctrl+Shift+X/A) runs after the window's accelerators, so it never sees these keys, and GtkSourceView's `move-viewport` stays shadowed as long as they are installed.
- **Keys column mode handles itself**, in a capture-phase key controller on the view that runs before GtkSourceView's own key handler and its input method: Escape and plain arrows leave column mode at the cursor corner; Tab types a tab on every line (spaces to the next indentation stop with "insert spaces"); Shift+Tab leaves column mode with the corners selected and unindents those lines; Enter leaves column mode and breaks the line at the cursor corner. Backspace, Delete and the clipboard keys stay GtkTextView's bindings, whose class handlers `StetView` overrides. Shift+arrows, Home, End and the other movement keys leave column mode at the cursor corner and then move as usual.
- **Mouse:** Alt+drag selects a rectangle; Alt+click puts a column caret; Alt+Shift+click moves the cursor corner, from the caret when column mode is off; a plain click leaves column mode.
- **Burst closer** ([ADR-016 amendment](#adr-016-amendment--as-built-in-m6-2026-10-01)): a capture-phase controller on the window, added after the application's shortcut controller so that it runs first, closes an open typing burst on every Ctrl, Alt, Super, Meta or Hyper chord, on navigation keys and Escape, and on every button press. AltGr is not a chord: it types characters.

**Why these keys.** They are the column-mode keys many users already know: the Column Editor on Alt+C, Begin/End Select in Column Mode on Alt+Shift+B, and Scintilla's `SCI_*RECTEXTEND` commands on Alt+Shift with the arrows, Home (`VCHOME`, so Home goes to the indentation, then to column 1), End, PgUp and PgDn. None is an Omarchy non-SUPER bind or an fcitx5 hotkey; the registry's unit tests (no key twice, grabs avoided, punctuation rules) pass with them. Alt+C and Alt+Shift+B are letters and Alt+Shift+arrows don't depend on the layout.

**Consequence:** The live session has to confirm that the keys reach the app on the Norwegian layout, and that Alt+drag reaches it under Hyprland (Omarchy moves windows with SUPER+drag, not Alt).

**Merged with M5 (2026-10-01):** M5's editor-scoped controller also lists GtkSourceView's `move-viewport` keys among the overridden built-ins it swallows. Both stand: the window's application accelerators run first in the capture phase, so Alt+Shift+arrows reach the column actions, and if one of them is unbound in `keys.toml` the editor's controller still keeps the key from GtkSourceView.

### ADR-009 amendment — Navigation keys (M8, 2026-10-01)

**Status:** Accepted — M8, under the standing authorization of 2026-10-01.

**Decision:**

| Action | Stet keys |
| --- | --- |
| Quick Open… | Ctrl+P |
| Next Recent Tab, Previous Recent Tab (the switcher) | Ctrl+Tab, Ctrl+Shift+Tab |
| Next Tab, Previous Tab (strip order) | Ctrl+PgDn, Ctrl+PgUp |
| Go Back, Go Forward | Alt+Left, Alt+Right; the mouse's back and forward buttons (8, 9) |
| Pin Tab, Unpin Tab, Replace in Files | no keys: the tab strip's menu, the View menu, the files row, the palette |

- **Ctrl+Tab** moves from M1's Next and Previous Tab, which keep Ctrl+PgDn and Ctrl+PgUp, to the most-recently-used actions. A press with Ctrl held opens the switcher (it shows after 120 ms, so a quick tap never flashes it), Tab and Shift+Tab move, releasing Ctrl switches, Escape cancels, Enter switches at once; a quick tap goes to the previous tab. A press without Ctrl held (the palette, the menu) switches at once.
- **How the keys are taken over.** AdwTabView's own shortcuts have been off since M1 (`TabView::set_shortcuts` with none), so the tab view never handles Ctrl+Tab, Ctrl+Shift+Tab or Alt+1..9 itself. Ctrl+Tab, Ctrl+Shift+Tab, Alt+Left and Alt+Right are application accelerators from the registry; GTK runs those in the window's own shortcut controller in the capture phase, before any widget inside the window sees the key: the tab view, GtkSourceView's `move-words` binding on Alt+Left and Alt+Right, GtkWindow's Ctrl+Tab focus moves, and any editor-scoped capture controller. Releasing Ctrl, and Escape and Enter while the switcher shows, reach a key controller on the window in the capture phase (`Window::connect_nav`), which consumes nothing else. The mouse's buttons 8 and 9 reach a click gesture on the window, in the capture phase.
- **Why these keys.** Ctrl+P: Print, its usual meaning, is a 1.x item (scope control), and Ctrl+P is quick open in editors many users know (VS Code, Sublime Text); it is not among fcitx5's hotkeys (its Ctrl+Alt+P is a different chord) or Omarchy's non-SUPER Hyprland binds. Alt+Left and Alt+Right are the browsers' back and forward keys; this ADR already listed GtkSourceView's `move-words` on them among the built-ins Stet overrides. The registry's unit tests (no key twice, F-key-free alternatives, grabs avoided, punctuation rules) pass with the new keys; `navigation_keys_m8` checks the table.

**Consequence:** Ctrl+Tab no longer follows the strip's order; Ctrl+PgDn and Ctrl+PgUp do. With a text entry focused (the find bar) Alt+Left and Alt+Right go back and forward too, as application accelerators do; the entries have no binding of their own on them. The live session has to confirm that Ctrl+Tab's release, Alt+Left and Alt+Right, Ctrl+P and the mouse's side buttons reach the app.

**Merged with M5 and M6 (2026-10-01):** Pin Tab and Unpin Tab are the first section of M5's tab menu (`TAB_MENU`), each hidden while it doesn't apply; M8's own small tab menu is gone. M5's Close Others and Close to the Right skip pinned tabs as Close All does, and Go to and Select to Matching Brace (M5) are jumps that Go Back returns from.

### ADR-009 amendment — Marking, view and compare keys (M7, 2026-10-01)

**Status:** Accepted — M7, under the standing authorization of 2026-10-01.

**Decision:**

| Action | Stet keys (menus show the first) |
| --- | --- |
| Toggle Bookmark | Ctrl+Alt+K, Ctrl+F2 |
| Next Bookmark, Previous Bookmark | Ctrl+Alt+L, F2; Ctrl+Alt+J, Shift+F2 |
| Mark… | Ctrl+M |
| Jump Down › 1st–5th Style | Ctrl+1 to Ctrl+5 |
| Jump Up › 1st–5th Style | Ctrl+Alt+1 to Ctrl+Alt+5 |
| Jump Down, Jump Up › Find Mark Style | Alt+M, Alt+Shift+M |
| Focus on Another View | Ctrl+Alt+O, F8 |
| Compare, Compare with Clipboard, Compare with Saved Version, Clear Compare | Ctrl+Alt+C, Ctrl+Alt+M, Ctrl+Alt+D, Ctrl+Alt+X |
| Previous Difference, Next Difference | Alt+PgUp, Alt+PgDn |
| The bookmarked-line operations, Clear All Marks, Copy Marked Text, the style tokens and Clear Style, Move and Clone to Other View, the Synchronise Scrolling toggles, Compare with File, Ignore Whitespace, Ignore Case | no keys: the menus and the palette |

- **Keys most users already know**, where these commands have them.
- **F-key-free keys first.** The bookmark keys are those of the Bookmarks extension for VS Code (Ctrl+Alt+K, L, J); Ctrl+Alt+O is "other view".
- **No Shift with a digit.** GTK matches an accelerator against the key's symbol after Shift (`gdk_key_event_matches`), and Shift turns a digit into a symbol that differs by layout (Shift+1 is `!` on `us` and on `no`, Shift+0 is `)` on `us` and `=` on `no`), so `<Control><Shift>1` never matches. Jump Up takes Ctrl+Alt with the digit instead; the unit test `digits_are_never_keys_with_shift` keeps Shift away from digits. Ctrl+0 stays Reset Zoom (the F-key amendment above left this to M7), so the Find Mark Style jumps are Alt+M and Alt+Shift+M (M for Mark).
- **Checked:** the registry's unit tests (no key twice, F-key-free alternatives first, known grabs avoided, the punctuation rules, `marking_view_and_compare_keys_m7` for this table) pass. None of the keys is one of Omarchy's non-SUPER Hyprland binds or fcitx5's hotkeys. Outside Omarchy, some desktops lock the screen on Ctrl+Alt+L; F2 and the menus still reach Next Bookmark there.

**Consequence:** The Jump Up and Jump Down keys are editor keys, as the text tools' are (M5): they move the editor's selection and work while it has the focus, so Alt+M never takes a dialog's mnemonic. The others are application accelerators that work from the find bar too, as M4's search keys do; Ctrl+M from the find field opens the Mark row. The live session has to confirm that Ctrl+Alt with letters and digits reaches the app on the Norwegian layout (AltGr is a different key there).

## ADR-010 — Name and app-id (2026-09-30)

**Status:** Accepted — user decision, 2026-09-30: the name is **Stet** (see [the acceptance below](#adr-010-accepted--stet-2026-09-30)). It was proposed earlier the same day with the working name "Notes"; the working values below are kept as history.

**Decision (working values):**
- binary `notes`; crates `notes` (`app/`), `notes-domain` (`domain/`), `notes-infrastructure` (`infrastructure/`), `notes-spikes` (`spikes/`);
- release app-id `io.github.pixdevsapps.Notes`; debug app-id `io.github.pixdevsapps.Notes.Devel`;
- the D-Bus name and desktop-file ID equal the app-id; state under `$XDG_STATE_HOME/notes`.

Rules for the final choice:
- never an app-id matching `org.omarchy.*` or `TUI.*`: Omarchy tags those as terminals, and `SUPER+C/V` then send Ctrl+Insert / Shift+Insert;
- avoid an `oma*` prefix, which implies an official Omarchy app, unless proposed upstream.

Candidates from the architect draft: Jotter (recommended there), Tabula, Quire, Nib, or keeping Notes. Availability on the AUR, crates.io and GitHub has **not** been checked.

**Reason:**
- Scope critique M8: the name fixes the app-id (desktop file, `StartupWMClass`, the Hyprland snippets users paste, the D-Bus name), the backup location, the crate names and the PKGBUILD. Renaming after M2 means migrating users' unsaved drafts.
- A `.Devel` app-id for debug builds keeps `cargo run` from forwarding to an installed instance (technical critique, low).
- "Notes" is generic and suggests note-taking (Obsidian) rather than a text editor.

**Consequence:** M0 cannot exit until this ADR is Accepted. A rename touches the crates, the app-id constants, the desktop file, the D-Bus service and the documented snippets.

### ADR-010 accepted — Stet (2026-09-30)

**Status:** Accepted — user decision, 2026-09-30. It supersedes the working values above.

**Decision:** The product is named **Stet**. The identifiers are:

| Thing | Working value (M0) | Final |
| --- | --- | --- |
| Product and display name | Notes | Stet |
| Binary and Arch package | `notes` | `stet` |
| Crates | `notes`, `notes-domain`, `notes-infrastructure`, `notes-spikes` | `stet`, `stet-domain`, `stet-infrastructure`, `stet-spikes` (Rust paths `stet_domain`, `stet_infrastructure`, `stet_spikes`) |
| App-id, release / debug | `io.github.pixdevsapps.Notes` / `io.github.pixdevsapps.Notes.Devel` | `io.github.pixdevsapps.Stet` / `io.github.pixdevsapps.Stet.Devel` |
| Desktop file and D-Bus service | `io.github.pixdevsapps.Notes.*` | `io.github.pixdevsapps.Stet.desktop`; `/usr/share/dbus-1/services/io.github.pixdevsapps.Stet.service` |
| Environment variables | `NOTES_EXIT_AFTER_MS`, `NOTES_HEADLESS_TIMEOUT`, `NOTES_SPIKE_ALLOW_LIVE` | `STET_EXIT_AFTER_MS`, `STET_HEADLESS_TIMEOUT`, `STET_SPIKE_ALLOW_LIVE` |
| Style scheme ids and custom styles | `notes-omarchy-<hash>`, `notes:smart-highlight`, `notes:search-current` | `stet-omarchy-<hash>`, `stet:smart-highlight`, `stet:search-current` |
| Rectangular clipboard MIME type | `application/x-notes-column-block` | `application/x-stet-column-block` |
| XDG directories | `$XDG_STATE_HOME/notes`, `$XDG_CONFIG_HOME/notes`, `$XDG_CACHE_HOME/notes` | `$XDG_STATE_HOME/stet`, `$XDG_CONFIG_HOME/stet`, `$XDG_CACHE_HOME/stet` |
| Per-theme override | `current/theme/notes.toml`, `notes.toml.tpl.sample` | `current/theme/stet.toml`, `stet.toml.tpl.sample` |
| Commands in snippets | `notes`, `notes --wait`, `GIT_EDITOR="notes --wait"` | `stet`, `stet --wait`, `GIT_EDITOR="stet --wait"`, `SUDO_EDITOR="stet --wait"` |
| Hyprland focus pattern | `^io\.github\.pixdevsapps\.Notes$` | `^io\.github\.pixdevsapps\.Stet$` |
| LICENSE copyright holder | Notes contributors | Stet contributors |
| Future GitHub repository (not authorized) | `PixDevsApps/<Name>` | `PixDevsApps/Stet` |

The project folder stays `/home/fredrick/Projects/linux-apps/notes`.

**Reason:**
- The proofreading mark "stet" (Latin for "let it stand") cancels a correction. It fits the core promise of never losing your text.
- It keeps the rules above: no `org.omarchy.*` or `TUI.*` app-id, no `oma*` prefix.
- Availability, checked on 2026-09-30: no `stet` package in the Arch repositories or the AUR, and no `stet` command on the system. A `stet` crate exists on crates.io, which does not matter because our crates are `publish = false`. **Not checked:** trademarks, and GitHub organisation or repository names.
- The name is fixed before M2, so no user drafts need migrating.

**Consequence:**
- The crates, tools and docs were renamed on 2026-09-30. Records written before the decision keep the working name: the spike reports, the earlier M0 entry in PLAN.md and the TESTING.md evidence of that day, each with a note that points here.
- The session and backups live in `$XDG_STATE_HOME/stet/` ([ADR-006](#adr-006--plain-file-session-and-backups-2026-09-30)).
- Creating the GitHub repository `PixDevsApps/Stet` needs its own authorization in the PLAN.md authorization log.

## ADR-011 — D-Bus activation for single instance and `--wait` (2026-09-30)

**Status:** Proposed. S6 measures it; single instance lands in M1, activation and `--wait` in M2.

**Decision:**
- Ship `/usr/share/dbus-1/services/<app-id>.service` (`Exec=/usr/bin/stet --gapplication-service`), set `DBusActivatable=true` in the desktop file, use `HANDLES_OPEN`, and call `StartServiceByName` before registering. The command line is then always a *remote*, and `--wait` blocks only until its own tab closes.
- If a `--wait` tab closes unsaved, or the app quits while it is dirty, exit with status 1 so git aborts instead of committing a stale message. `--wait` files are excluded from the session.
- Parse arguments locally before `run()`; the primary uses `try_get_matches_from` and reports errors to the remote. A typo in a second terminal must never `process::exit` the running editor.
- `glib::unix_signal_add_local` handles TERM, HUP and INT by flushing backups and quitting; uwsm sends SIGTERM at logout.

**Reason:**
- Technical critique #1: GApplication blocks only for *remote* invocations. If Stet isn't running, `git commit` would become the primary and wait for the whole editor to quit; the editor would inherit git's environment and working directory, live in the terminal's scope, and die on SIGHUP when the terminal closes, losing up to 7 s of edits.
- Technical critique #4: clap's `get_matches_from` calls `process::exit` on `--help`, `--version` or any error, which in the primary would kill unsaved work.
- Technical critique #13: uwsm stops app scopes with SIGTERM at logout and reboot; GTK apps die on it without flushing.
- Omarchy #13037: GUI editors break `git commit` because `omarchy-launch-editor` runs them detached with `setsid uwsm-app` and passes no wait flag.

**Alternatives:** A Unix socket in `$XDG_RUNTIME_DIR`; `zbus`. GApplication already covers D-Bus, so neither is needed.

**Consequence:** Packaging installs the service file. The M2 exit test covers `--wait` with Stet both running and not running. How development builds behave without an installed service file is settled in S6.

**References:** [GApplicationCommandLine](https://docs.gtk.org/gio/class.ApplicationCommandLine.html), [Omarchy #13037](https://github.com/omacom/omarchy/issues/13037).

### ADR-011 note — Single instance in M1 (2026-10-01)

**Status:** Implemented in part, in M1; D-Bus activation, the service file and `--wait` remain M2.

- The app runs with `HANDLES_COMMAND_LINE`. `main` parses the arguments locally first with clap (so `--help`, `--version` and typos never reach a running editor), and the primary parses every forwarded command line again with `try_get_matches_from`, printing errors to the calling terminal through `GApplicationCommandLine` and returning status 2. Nothing in the primary calls `process::exit`.
- Supported: `stet file`, `file:line`, `file:line:col` (split off only when the literal path does not exist), `-n <line>`, `-c <col>`, `--version`, `--help`, `--self-test <script>`.
- The command-line object is held until its files are open, which is where M2's `--wait` hooks in.
- Test hook: `STET_APP_ID` overrides the application id, so the self-test and the second `stet` processes it starts share an id no real Stet uses.
- Verified headless on 2026-10-01 by `tests/selftest/cli.stet-test`, which starts real second processes against the running instance ([TESTING.md](TESTING.md)).

### ADR-011 amendment — D-Bus activation and `--wait` as built in M2 (2026-10-01)

**Status:** Accepted — M2, under the standing authorization of 2026-10-01. It settles what S6 left open: how a command line reaches a primary when none runs, and what development builds do without an installed service file.

**Decision:**
- The package installs `/usr/share/dbus-1/services/io.github.pixdevsapps.Stet.service` (`Name=io.github.pixdevsapps.Stet`, `Exec=/usr/bin/stet --gapplication-service`), and the desktop file sets `DBusActivatable=true`. The app runs with `HANDLES_COMMAND_LINE` and `HANDLES_OPEN`: launchers and file managers that activate it over D-Bus reach `activate` and `open`; a local command line still goes through `command-line`. clap accepts `--gapplication-service` (hidden), and GApplication handles it.
- **Before the application registers** (`app/src/activation.rs`), every command line except `--gapplication-service` and `--self-test` asks the session bus whether the application id has an owner. If not, it calls `StartServiceByName` (10 s), which answers once the started service owns the name, and then runs as a remote. With dbus-broker (Omarchy's session bus) the service runs as a transient systemd user unit with the user manager's environment, outside the terminal's scope.
- **Without a service file** (`org.freedesktop.DBus.Error.ServiceUnknown`: a development build, whose id is `io.github.pixdevsapps.Stet.Devel`, or an install without it): a `--wait` command line starts `<its own executable> --gapplication-service` itself, in its own process group, with no terminal input or output and the home directory as working directory, waits up to 10 s for it to own the name, and runs as its remote. Any other command line becomes the primary, as in M1. So does every command line without a session bus, or when activation fails; `--wait` then waits for the whole editor, as GApplication alone would.
- **`--wait`:** the primary keeps the `ApplicationCommandLine` until every tab the command line opened, or selected because its file was open already, has closed. The exit status is 1 when one of them closed with unsaved changes (Discard) or did not load, or when the window goes while one has unsaved changes; otherwise 0. Quitting, closing the window and the signals answer every waiting command line. Without files it returns at once. `--wait` tabs stay out of the session and out of the recent files.
- **`--no-session`** applies only to the command line that starts Stet; a running Stet prints a note to the calling terminal and keeps its session. **`--new-window` is not built:** Stet has one window.
- Quit with no window (a service that has not shown one yet) quits.

**Reason:** GApplication blocks only remote command lines, so `stet --wait` must never become the primary (technical critique #1); activation by the bus also keeps the editor alive when the terminal it was started from closes. The fallback spawns only for `--wait`, because a plain development `stet file` that becomes the editor keeps its log in the terminal, as before, and the self-tests' and CI's private buses have no service files by design.

**Consequence:** `tests/selftest/activation.stet-test` runs it on a private `dbus-daemon` whose only service directory holds Stet's own test service file, or nothing ([TESTING.md](TESTING.md#2026-10-01--m2-sessions-backups-restore-and---wait)): status 0 and 1 as above, through the real `--gapplication-service` and through the fallback. GTK exports no window actions over D-Bus under Broadway (only its Wayland and X11 backends do), so the editing half of that test runs a self-test as the activated service. The live `GIT_EDITOR` and `SUDO_EDITOR` checks, with an installed service file, are in [PLAN.md](PLAN.md#m2-built-and-checked-headless-live-checks-pending--2026-10-01).

## ADR-012 — Repository docs as canonical project memory (2026-09-30)

**Status:** Accepted — user decision, 2026-09-30.

**Decision:** The Markdown in this repository (`docs/` plus the root README, AGENTS and CONTRIBUTING) is the only project memory. There is no Notion project and no Kanban mirror. Status, blockers, follow-ups, authorizations and preview gates are dated entries in [PLAN.md](PLAN.md); decisions live here; evidence lives in [TESTING.md](TESTING.md) and `docs/spikes/`.

**Reason:**
- The user chose repository docs only.
- The Notion connector was not authenticated in the planning session (scope critique M9), and Hertz's rule "stop if Notion is unavailable" would have blocked work.
- One source of truth avoids the drift that Hertz's mirror headers ("Synced", "Source last edited") exist to manage.

**Consequence:** AGENTS.md drops Hertz's Notion steps. Every material change updates the affected docs in the same change. Follow-ups are recorded in PLAN.md instead of Kanban cards.

## ADR-013 — Lean 1.0 scope (2026-09-30)

**Status:** Accepted — user decision, 2026-09-30.

**Decision:** 1.0 is the MVP (0.3) plus column mode with the Column Editor; bookmarks, Mark with bookmarked-line operations and style tokens; split view, clone and compare; quick open, the MRU switcher, back/forward, pinned tabs and first-line tab names; and Replace in Files. Everything else is on the 1.x, Later or Never lists in [PLAN.md](PLAN.md#scope-control).

**Reason:**
- Scope critique H1: the draft's M6–M8 added about 28–40 days of less-used features (macros, Run, tail -f, a pkexec helper, hash tools, completion, a Preferences GUI, a second keymap preset, OEM code pages, folding, multi-caret), close to a complete feature set, against the brief of "the most commonly used ones".
- Scope critique H2: column mode is a feature people say they can't do without (SO 1.33M "add to every line"), so it comes first after the MVP, and prefix/suffix and numbering tools ship in the MVP.
- Scope critique M6: a save-as-administrator helper is the largest security surface for modest value; `SUDO_EDITOR="stet --wait" sudoedit` covers it.
- Scope critique M7: a Preferences GUI doesn't fit Omarchy, where settings live in files; the palette opens `config.toml` and `keys.toml`.
- Stet competes on being native to Omarchy, on reliability and on focus, not on the number of features.

**Consequence:** About 48–64 focused days to the MVP and 78–106 to 1.0. Moving a feature between tiers needs a user decision, recorded as a dated PLAN.md entry and an amendment here.

### ADR-013 amendment — Two additions after 1.0 (2026-10-01)

**Status:** Accepted — user decision, 2026-10-01.

**Decision:** After installing 1.0.0 the user asked for a double-click on a tab to rename its file, then for Save and Save As… in the tab menu. Both are in 1.1.0. Neither was on the 1.x, Later or Never lists; each reuses an existing command (Rename File…, Save, Save As…).

**Reason:** The user's requests in chat ([PLAN.md](PLAN.md#stet-110-double-click-a-tab-to-rename-save-in-the-tab-menu--2026-10-01)).

**Consequence:** The double-click relies on libadwaita's internal tab widget (`AdwTab` and its `page` property; [DESIGN.md](DESIGN.md#as-built-in-11--2026-10-01)). A libadwaita update that changes it turns the double-click into plain clicks without other harm, and the self-test `tab-double-click` fails. The 1.x list is unchanged.

### ADR-013 amendment — Names for untitled tabs (1.2, 2026-10-01)

**Status:** Accepted — user decision, 2026-10-01.

**Decision:** After 1.1 the user asked for custom tab names without saving: "That would make it easier for the user when having multiple tabs open and give them names, without going through the save/save as dialog for each of them". In 1.2.0, Rename… (Rename File… until 1.1) names an untitled tab and saves nothing; on a file it still renames the file. A double-click on an untitled tab opens it, in place of 1.1's Save As….

**Reason:** The user's request ([PLAN.md](PLAN.md#stet-120-names-for-untitled-tabs-without-saving--2026-10-01)).

**Consequence:** One more field in the session ([ADR-006 amendment](#adr-006-amendment--names-for-untitled-tabs-12-2026-10-01)). Files keep their file name in the tab; giving a file's tab another name is not part of this. The 1.x list is unchanged.

## ADR-014 — Syntax-highlighting size policy (2026-09-30)

**Status:** Accepted — user decision, 2026-09-30, on the evidence of spike S1. The threshold value is pending the M3 measurement.

**Decision:**
- Syntax highlighting is switched off above a size threshold measured in **both bytes and lines**, and/or it starts after a short delay once a file has opened. This replaces the previous rule, under which highlighting stayed on up to the 50 MB large-file threshold. Whether the threshold, the delay or both are needed is settled with the value in M3.
- The initial default is set by an S1 follow-up measurement in M3: repeat the right-after-load typing series at 1, 2 and 5 MB and at 10–20 MB. Until then the working assumption is a threshold of roughly 10–20 MB.
- The formatted view of the long-line path falls under the same rule.

**Reason** (S1, headless under Broadway, 2026-09-30):
- Highlighting costs about 0.4–0.5 KiB of RSS per line. Opening 100 MB took 7.04× the file size in RAM with Rust highlighting, against the 3× bar; without highlighting it was 2.67×.
- After a 10 MB Rust file opened, typing lagged for about 7 s while the initial highlighting pass ran: median 80–104 ms, p95 up to 1.8 s, and no frame painted for up to 2.05 s. Typing without highlighting stayed at 2.5–6.4 ms median.
- The formatted 10 MB single-line JSON (836k lines) took 645 MiB once highlighted.

**Consequence:**
- M3 carries the measurement and the policy; the measured default is recorded as a dated amendment here.
- The rest of large-file mode (from 50 MB, capped at 256 MB) is unchanged by this ADR.
- These are Broadway numbers. Whether the post-open stall reproduces on Hyprland is part of the live re-measure that remains in M0.

### ADR-014 amendment — The limits are 2 MiB and 100,000 lines (2026-10-01)

**Status:** Accepted — M3, under the standing authorization of 2026-10-01, on the measurement the ADR asked for ([S1 addendum](spikes/S1-performance.md#addendum-the-adr-014-follow-up-m3-2026-10-01)).

**Decision:**
- A document gets syntax highlighting when it opens if it has at most **2 MiB** (2,097,152 bytes of text) **and at most 100,000 lines** (`stet_domain::view::highlight_by_default`). There is **no delayed start**.
- Above the limits **the buffer gets no language at all**; the language is still detected and shown in the status bar, and the picker still sets it. A banner says why and offers **Highlight Anyway**.
- Large-file mode (from 50 MiB) never highlights and has no Highlight Anyway.
- The formatted view of the long-line path follows the same rule: the 10 MiB single-line JSON of the self-test formats to 20.3 MB and opens without highlighting.

**Reason** (headless under Broadway, 2026-10-01; right after loading, 50 keystrokes per run, 3 runs):
- Rust text typed at 7.5 ms median, 60 ms at worst, at 2 MB (38,509 lines); 35 ms median at 3 MB; and from 4 MB (76,644 lines) frames were held off for 0.5–1.5 s (5 MB: p95 815 ms, max 1.22 s; 20 MB: max 1.51 s).
- At the same size, more lines cost more: 2 MB with 119,195 or 531,515 short lines typed at 28–29 ms median, without stalls; 1 MB with 74,298 lines at 4 ms, with 265,651 at 24 ms. 100,000 lines keeps the line count where the short-line runs stayed fast.
- A delayed start moves the first pass instead of avoiding it: started 1 s after loading, 5–10 MB still typed at about 50 ms median, up to 0.2 s, for the 5–8 s the pass took.
- Detaching the language is necessary: with the language set and `highlight-syntax` off, which GtkSourceView documents as tags only, 10 MB stalled just as with highlighting on (frames held off up to 1.14 s). Its context engine analyses the whole buffer either way, from an idle at `G_PRIORITY_HIGH_IDLE`, above GDK's redraw priority.

**Consequence:**
- In the app, typing 30 keys into a 10 MB Rust file right after it opened took 3.4 ms median, 11.7 ms at worst (`large.stet-test`, release build); with Highlight Anyway the same file typed at 41.9 ms median and 325 ms at worst (`typing-control.stet-test`). Headless numbers; the live check is on the M3 list in [PLAN.md](PLAN.md).
- Comment toggling (M5) and other language features must use the document's language, not the buffer's.
- Revisit with GtkSourceView 5.22 or when the live measurements disagree.

### ADR-014 note — The limits are settings (M5, 2026-10-01)

`highlight_max_bytes` and `highlight_max_lines` in `config.toml` ([ADR-017](#adr-017--settings-in-configtoml-and-keystoml-2026-10-01)) replace the two limits for files opened afterwards; the defaults stay 2 MiB and 100,000 lines. Large-file mode (50 MiB) is not a setting.

## ADR-015 — Exact tab stops in Pango units (2026-09-30)

**Status:** Accepted — user decision, 2026-09-30, on the evidence of spike S5. Implemented in M1.

**Decision:**
- Set the view's tab stops with `set_tabs` in Pango units: the tab width times the font's exact advance (for example 4 × 9011 = 36044), instead of GtkSourceView's whole-pixel stops.
- Re-apply them on `notify::tab-width` and whenever the font or the zoom changes.
- A headless test compares the `iter_location` x after a tab with the character grid.
- It lands in M1, in the editor setup, because it affects every user whose font advance is fractional, not only column mode.

**Reason** (S5, headless under Broadway, 2026-09-30):
- GtkSourceView used a 36 px stop where 4 columns are 35.2 px (JetBrainsMono Nerd Font at 14.667 px: an 8.80 px advance, 9011 Pango units). After `abcdefgh` a tab drew less than 2 px wide, and rectangle edges on lines with tabs were 1–34 px off.
- The Pango-unit stop fixed every position (0 of 18 off).
- GtkSourceView resets its own tabs when `tab-width` changes (measured). Font changes were not tested and probably need the same re-application.

**Consequence:** The advance is read from the current font rather than hard-coded; 9011 is the value for that font and size. Column mode ([ADR-016](#adr-016--column-mode-editing-and-undo-model-2026-09-30)) relies on exact stops.

### ADR-015 amendment — Re-applied after GtkSourceView's own resets (2026-10-01)

**Status:** Accepted — M1, under the standing authorization of 2026-10-01.

**Decision:** The window checks the visible editor in every layout phase of its frame clock and re-applies the Pango-unit stops when GtkSourceView replaced them (its stops are in pixels) or when the font (Pango context serial) or the tab width changed (`app/src/tabstops.rs`).

**Reason:** GtkSourceView 5.20's `gtk_source_view_css_changed` rewrites its whole-pixel stops on **every** CSS change of the view once a tab width was set, not only on font changes: focus, hover and theme changes too (read in the 5.20.0 source on 2026-10-01). The first M1 build re-applied only on font and tab-width changes, and the tab-stop self-test failed on the first file it opened (pixel stops were back). With the check it passes: after a tab the next character lies on the grid within 0.41 px at 11, 13 and 10 pt and at tab widths 4, 8 and 3 (JetBrainsMono Nerd Font, advance 8.8 px = 9011 Pango units at 11 pt; headless, 2026-10-01).

**Consequence:** Every such CSS change invalidates the text layout twice (GtkSourceView's reset, then ours). That is invisible on small files; M3's large-file work re-measures it.

## ADR-016 — Column-mode editing and undo model (2026-09-30)

**Status:** Accepted — user decision, 2026-09-30, on the evidence of spike S5. Implemented in M6.

**Decision:**
- Column typing holds one user action (undo group) open across a typing burst, so a burst undoes in one step.
- The group is closed before anything else acts:
  - by a capture-phase key controller, on any Ctrl, Alt or Super chord, navigation key or Escape;
  - on focus-out;
  - on a click;
  - when a menu or the palette opens;
  - when column mode ends;
  - after a short idle timeout.
- Fallback if the grouping proves fragile: one undo step per keystroke, which is functional.
- Large rectangles use the bulk-edit rule of the [ADR-003 amendment](#adr-003-amendment--bulk-edits-by-edit-count-2026-09-30).
- The rectangular clipboard uses the custom MIME type `application/x-stet-column-block`, with a plain-text fallback.

**Reason** (S5, headless under Broadway, 2026-09-30):
- Replication works: typing, Backspace and paste replicate through `insert-text` and `delete-range`, at about 34 ms per keystroke with a 10k-line caret.
- Plain replication gives one undo step per key: 5 replicated keystrokes took 5 steps to undo.
- A group held open gives exactly one step (5 keys plus Backspace undid in one), but while it is open `can_undo` is false and `undo()` does nothing, so it must be closed before any other action.
- Undoing a 10k-line column edit made of per-line edits took about 250 ms with a view attached; a range rewrite undid in 4.5 ms.
- A union `ContentProvider` offering the custom type and `text/plain` worked; plain-text clipboards fall back to GTK's own paste.

**Consequence:**
- M6 exit criteria: a column typing burst undoes in one step, with the per-keystroke fallback documented here; undo and redo of a 10k-line column edit take less than 200 ms each, through the bulk-edit rule.
- fcitx5 preedit and commit in column mode are not verified yet; they need the live session.
- S5's open design points (tab snapping or splitting, typing over a block, wide characters) are settled in M6.

### ADR-016 amendment — As built in M6 (2026-10-01)

**Status:** Implemented in M6 under the standing authorization of 2026-10-01. The burst grouping held, so the per-keystroke fallback is not used. It settles S5's open design points.

**Decision, as built:**
- **The rectangle** is an anchor and a cursor in visual columns (`stet_domain::column`), virtual space allowed (up to column 65,536), kept by `StetView`, a GtkSourceView subclass that every editor tab uses. While column mode is on, the GTK selection is empty and GTK's caret is hidden at the cursor corner (clamped to the text); the view paints the rectangle ([DESIGN.md](DESIGN.md#as-built-in-m6--2026-10-01)).
- **A typing burst** opens a user action at its first key and holds it open. It is closed:
  - by a capture-phase controller on the window that runs before the application accelerators: on every Ctrl, Alt, Super, Meta or Hyper chord (AltGr is not one), on arrows, Home, End, PgUp, PgDn (and the keypad's) and Escape, and on every button or touch press;
  - before every registry action, whether from a key, the menu or the palette (`column::before_action`);
  - when the view loses the focus (palette, menu, dialog, another widget);
  - when column mode ends, and when the view is destroyed;
  - after 1 s without a keystroke (`BURST_IDLE`).

  Opening the menu or the palette needs no hook of its own: the press or key that opens them and the focus leaving the view close the burst first. `column.stet-test` checks each trigger (two keys around it make two undo steps; a burst makes one).
- **Typed text** reaches the buffer as GtkTextView's commit, `insert-text` at the caret inside its user action, whatever produced it: a plain key, a dead key or compose sequence, or an input method such as fcitx5. A handler connected before GtkTextView's stops that emission and types the text on every line. Text with a line break leaves column mode and goes in at the caret. fcitx5's preedit is GtkTextView's own, shown at GTK's caret (the cursor corner, or the end of its line when the corner is in virtual space) until the commit. An input method that deletes around the caret (`delete-surrounding`) ends column mode first.
- **A tab across the column is split, not snapped:** when an edit's column falls strictly inside a tab, that tab becomes the spaces it is drawn as, so the edit lands exactly on the column and nothing before it moves. Vim's blockwise operators and Emacs's rectangles do the same; Scintilla snaps each line to a character boundary instead, which puts typed text left of the painted caret. Tabs that start or end at the column stay tabs.
- **Wide characters and combining marks take one cell each,** as GtkSourceView's visual column and the status bar count them, so edits always cover whole characters; the painted rectangle follows the glyphs and has a ragged edge on such lines.
- **Typing over a block replaces it** on every line. Backspace and Delete on a block delete it. On a column caret they delete the character before (or after) the caret on every line when those characters are equally wide, so a tab typed in column mode goes in one keystroke; otherwise one cell, splitting a tab. Neither joins lines; in virtual space Backspace only moves the caret and Delete does nothing.
- **Overwrite mode:** on a column caret, typed text replaces as many cells after the caret as it takes, on every line; on a block, typing replaces the block as in insert mode. GtkTextView's own overwrite delete of one character is dropped.
- **Tab** types a tab on every line, or spaces to the next indentation stop with "insert spaces". **Enter** leaves column mode and breaks only the cursor corner's line; column edits never add line breaks.
- **The rectangular clipboard:** Copy and Cut offer the block as `application/x-stet-column-block` (the rows joined by `\n`) and the same text as `text/plain`, through a union content provider; Copy of a column caret leaves the clipboard alone. A block pastes as a rectangle: in column mode at the rectangle's left column from its top line, replacing its cells, rows padded so the text to their right stays aligned, lines appended past the end of the text; at a normal caret from the caret's column, replacing the selection, as one undo step, with the caret after the first row and column mode off. **Plain text** from another application pastes into a rectangle: one line (one final line break ignored) is typed on every row, in place of the block if there is one; several lines go in as a block, whatever the number of rows, with CR LF and CR counted as line breaks. Outside column mode, plain text is GTK's own paste.
- **Leaving column mode:** Escape, a plain arrow or a click leave the caret at the cursor corner; Undo and Redo leave it where GTK puts it; any other registry action that doesn't know columns (Find, Select All, Go to Line, …) leaves the two corners selected, so it acts on the rectangle's lines; so does Shift+Tab; any insertion, deletion or caret move that column mode didn't make ends it. Copy, Cut, Paste, Delete, the save commands, zoom, view toggles, the palette, tab navigation and the column actions keep it.
- **The Column Editor** (Alt+C): text or numbers (initial, increase by, repeat, leading none, zeros or spaces, Dec, Hex with optional uppercase, Oct, Bin, the number fields read in the chosen format), written on the rectangle's lines, or with only a caret from the caret's line to the end of the text at its column; one undo step; errors in the dialog, which stays open.
- **Large rectangles** follow the bulk-edit rule as amended for M6 ([ADR-003 amendment](#adr-003-amendment--large-column-edits-keep-the-view-m6-2026-10-01)).
- **Undo after Redo:** GTK 4.22 joins the next edit to a step of several edits that was undone and redone ([upstream note](upstream/gtk-text-history-redo-barrier.md)). The first M6 build showed it: a column burst after Undo and Redo was undone together with the redone step. Every editor buffer now closes the redone step again: at once when nothing is left to redo, otherwise as the next user action begins.
- **Accessibility:** `StetView` re-implements `GtkAccessibleText.get_selection`, NULL-safe, and leaves the rest of the interface to GtkTextView, so AT-SPI's `AddSelection` no longer reaches GTK 4.22's crash ([upstream note](upstream/gtk-accessible-text-add-selection.md)). While column mode is on, assistive technologies see the caret at the cursor corner and no selection; the rectangle itself is not exposed.

**Reason:** S5's evidence above. The triggers as built cover every way a key, a click, the menu or the palette can act while a burst is open, and the self-tests check them one by one.

**Consequence:**
- Exit criteria, headless: a burst undoes in one step; inserting at column 0 on 10,000 lines is one undo step in 14.1–15.2 ms (release build), and its undo and redo take 24.7–27.7 ms; typed text through `insert-text` goes in on every line. fcitx5 typing in column mode needs the live session ([PLAN.md](PLAN.md#m6-column-mode-built-and-checked-headless-live-checks-pending--2026-10-01)).
- The primary selection (middle-click paste) is not set from a rectangle.

## ADR-017 — Settings in config.toml and keys.toml (2026-10-01)

**Status:** Accepted — M5, under the standing authorization of 2026-10-01 (the PRD's "config and keymap files, `config.toml` and `keys.toml`, with hot reload"; ADR-013 leaves a Preferences GUI to 1.x).

**Decision:**
- **Two TOML files in `$XDG_CONFIG_HOME/stet/`.** `config.toml` holds flat settings (`font`, `font_size`, `tab_width`, `insert_spaces`, `detect_indentation`, `word_wrap`, `show_whitespace`, `document_map`, `smart_highlight`, `word_count`, `backups`, `backup_interval`, `highlight_max_bytes`, `highlight_max_lines`) and `[language.<GtkSourceView id>]` tables with `tab_width` and `insert_spaces`. `keys.toml` maps action names (the registry's stable GAction names) to lists of GTK accelerators, `action = ["<Control>d"]`, `[]` to unbind (`stet_domain::settings`).
- **Read with positions, applied whole.** The files are parsed with the `toml` crate's spanned tables (`toml::de::DeTable`; features `std` and `parse` only), so every mistake has a line and column: syntax, an unknown setting or action (with a "did you mean"), a value of the wrong type or out of range, a key GTK can't parse, a key without Ctrl, Alt or Super (only F-keys may), the same key twice, a key another action already has (its default or another line), the text view's own editing keys. A file with any mistake is not applied at all; the previous settings stay and a toast names the first mistake ("keys.toml not applied: line 1, column 33: …"). A missing file is the defaults.
- **Hot reload.** File monitors on the directory and on its parent (so a directory created or replaced later is noticed; M1's `DirWatch`) reload both files on a worker 150 ms after either changes, from Stet or any other program; an unchanged file is not applied or reported again. A keymap change reinstalls the accelerators, refills the editors' shortcut list and rebuilds the menus; a settings change applies the font, the indentation of documents that follow the defaults, smart highlighting and the word count, and a view default (`word_wrap`, `show_whitespace`, `document_map`) when that setting itself changed, so the View menu's toggles last until then.
- **Precedence for indentation:** the global settings, then Stet's defaults per language (Makefile and Go tabs; Python and Rust four spaces; YAML, JSON and XML two), then `[language.<id>]`; a file's own detected indentation wins when it opens (`detect_indentation`), and the status bar's popover wins over all for its document.
- **Opened from the Settings menu and the palette, with the defaults written out.** Open Settings and Open Keyboard Shortcuts create the file when it is missing, with every setting and its default, or every rebindable action and its default keys, as comments (generated from `Settings::default()` and the registry, so the files stay complete; unit tests check that the generated files parse and change nothing), and open it in a tab. An existing file is never rewritten.
- **For M2:** `backups` and `backup_interval` (seconds, 1–3600, default 7) are read and exposed as `Settings::backup_every()`; M2's backup timer uses them.

**Reason:**
- Omarchy configures everything through files, and ADR-013 keeps a Preferences GUI out of 1.0; the PRD names both files.
- TOML is what Omarchy's own `colors.toml` and the Rust ecosystem use; positions in errors make hand-edited files safe to get wrong.
- Applying a file only when it has no mistakes keeps a half-edited `keys.toml` from leaving a keymap that is neither the old nor the new one.

**Alternatives:** a TOML subset parser of our own (no dependency, but a second dialect of a standard format); serde derive with `deny_unknown_fields` (positions for the first error only); GSettings (not a file users edit).

**Consequence:** `toml` 1.1.6, already locked as a build dependency of the GTK `-sys` crates (through `system-deps`), now ships in the binary with `toml_parser`, `toml_datetime` (both MIT or Apache-2.0) and `winnow` (MIT); `Cargo.lock` gained no package, and the license bundle records them. Unit tests cover both parsers (every setting, every mistake, positions, the default files being valid and changing nothing); the self-test `settings.stet-test` covers overrides, conflicts, hot reload and the palette commands.

## ADR-018 — Split view and compare presentation (M7, 2026-10-01)

**Status:** Accepted — M7, under the standing authorization of 2026-10-01 (the plan's M7: "split, clone and move view; synchronized scrolling; compare").

**Decision:**
- **Two views, shown only when they have tabs.** Each view is an AdwTabView with its own AdwTabBar; the strips sit side by side above the views, and two GtkPaned (one for the strips, one for the views) are bound to one position, so the strips and the views divide at the same place. A view hides when its last tab goes; one view looks exactly as before. The active view is where commands act, new tabs open and the status bar and title look: the view that last took the keyboard focus or a click on its strip. Moving a tab uses AdwTabView's `transfer_page`; the session restores the views ([ADR-006 amendment](#adr-006-amendment--split-view-clones-and-bookmarks-m7-2026-10-01)).
- **A clone shares the document:** a second EditorPage over the same `Document` and GtkSourceBuffer, with a view, banners and a scroll position of its own. The buffer has one caret and one selection, so each view keeps its own in two marks while another view of the document is in front, and the view in front puts its own back (`EditorPage::take_caret`). Only one view of a buffer is in column mode at a time. Operations over documents (saving, searching or replacing in open documents, quick open, the closing prompts) count a document once (`Window::documents`); closing a clone never asks.
- **Compare** diffs on a worker (`domain::diff::compare`: imara-diff's histogram algorithm with git's indent heuristic, paired changed lines, inline word differences, moved blocks) and presents each side in its own document: `paragraph-background` tags for whole lines (GTK skips that background on a line without characters, so the view paints empty lines itself), a stronger background tag for the characters that differ, and padding where the other side has lines this one lacks, so equal lines face each other at the same height: `pixels-below-lines` on the line before the gap, or the view's top or bottom margin, painted in the padding colour by the view. Word wrap is off while compared, and the two views scroll level. The document in front is the new version and the other the old one; lines only in the new one are "added". Texts the comparison opens (a chosen file, the clipboard, the saved version) are read-only temporary documents in the other view, outside the session and the recent files, closed with the comparison. An edit compares again 300 ms later; results for texts that changed meanwhile are dropped.

**Alternatives:**
- One TabView with a second "pane" per page: AdwTabView shows one page at a time, so two documents side by side need two tab views.
- Two buffers kept in sync for a clone: every edit twice, two undo histories that disagree, and M2–M6's per-document state (backups, search, column mode) doubled; one buffer shows the document once by construction.
- Padding with inserted blank lines (as editors on Scintilla do with annotations): it changes the text, undo history and line numbers; a tag's `pixels-below-lines` changes only the layout.
- A separate diff window (Meld-style): comparing in the two views keeps the documents editable.

**Consequence:** Comparing two generated 10,000-line texts (11,878 and 11,860 lines) takes 7.6–8.3 ms end to end in the release build right after opening them, 44.5–47.8 ms when Compare moves a laid-out document to the other view, 11.0–11.7 ms again after an edit, and 64.0–67.4 ms after a Replace All changed nearly every line; the first painted frame follows within 100 ms, except right after opening the files, when it took up to 1.33 s (headless, [TESTING.md](TESTING.md#2026-10-01--m7-marking-split-view-and-compare)); every row with a line on both sides is level to the pixel in the self-tests. Both views of a buffer draw its selection and current line (GTK draws the buffer's), and the inactive view of a document's clone shows the active view's selection; GtkSourceView's search highlighting is per buffer, so it shows in both views of a document.
