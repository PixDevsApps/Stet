# Architecture

Project: Stet, a fast, keyboard-first text and code editor for Omarchy

Synced: 2026-11-13

How the code is organised and why, for anyone about to change it. The rules behind it are [ADR-020](DECISIONS.md#adr-020--code-architecture-layers-bounded-contexts-and-lints-2026-11-13); the product decisions are the other ADRs in [DECISIONS.md](DECISIONS.md). Build and test commands are in [BUILD.md](BUILD.md) and [TESTING.md](TESTING.md).

---

## Three crates, one direction

```
stet-domain  ←  stet-infrastructure  ←  stet (the app)
  pure logic       std I/O, headless       GTK 4, libadwaita, GtkSourceView 5
```

| Crate | Folder | May | May not |
| --- | --- | --- | --- |
| `stet-domain` | `domain/` | Model the editor and decide: text, search, sessions, settings, menus, banners. Parse and write data formats as pure functions over text or bytes. | Open a file, a socket or a process; start a thread; name GTK. |
| `stet-infrastructure` | `infrastructure/` | Read and write files, walk folders, run PCRE2 and ripgrep's searcher, detect and convert encodings, talk to Omarchy's files and the desktop. Every test runs without a display. | Link GTK. |
| `stet` | `app/` | Everything on screen, and the main loop. The only crate with `unsafe`, kept in `app/src/sys/`. | Do blocking work on the GTK thread. |

The domain keeps formats that are its models' own contracts: `session.json` *is* the session record with its forgiving decoding (ADR-006), and `config.toml`/`keys.toml` errors are reported by line and column (ADR-017). See the [ADR-020 amendment](DECISIONS.md#adr-020-amendment--formats-stay-with-their-models-2026-11-13).

## The domain's bounded contexts

`domain/src/lib.rs` lists them; each is one module with its own vocabulary.

| Context | Module | Holds |
| --- | --- | --- |
| Text | `text` | Character, byte and line offsets (`offset`), `TextEdit` and offset mapping, line endings, `Selection`, character rules (the NUL placeholder, word characters), count wording. |
| Document | `document` | Names and titles, untitled tabs, indentation, language hints, `file:line:col`, the document's read-only and loss state, and **which banners a tab shows** (`banners_for`). |
| Search | `search` | The query, the regex dialect translated to PCRE2, replacement templates, history, smart highlighting, report sentences, Replace in Files' rules. |
| Editing operations | `ops`, `column` | The text tools (case, lines, comments, numbers, JSON, XML) as pure functions returning edits; column mode's rectangles and edits (ADR-016). |
| Comparison | `diff` | Line alignment, moves and inline differences. |
| Annotations | `marks` | Bookmarks, marked lines, token styles and their history across edits. |
| Navigation | `nav` | Position history, the tab switcher's MRU, recent files, closed tabs, quick open's list. |
| Matching | `fuzzy` | The one fuzzy matcher, for quick open, the command palette and the pickers. |
| Session | `session` | The record `session.json` keeps, its ids (`RecordId`, `ViewSlot`) and the backup policy. |
| Settings | `settings` | `config.toml` (declared once per setting) and the keymap. |
| Commands | `actions` | The action registry (`ActionId`, one table), accelerators, and the menus as a `MenuTree`. |
| Appearance | `theme`, `view` | Colours, the Omarchy palette mapped to editor roles; zoom, font size, tab width, undo cap. |
| Updates | `updates` | Versions and the answer to Check for Updates. |

## Infrastructure

| Module | Does |
| --- | --- |
| `fs` | Reading a file for a tab (`read_file`, `DecodeReport`), saving it safely (`save`), metadata and fingerprints, size classes, and `durable`: the one way to write a file (temporary file, sync, rename, sync the folder). |
| `encoding` | Detection, decoding to an LF buffer with NUL mapped to the placeholder, encoding back (ADR-007, ADR-008). |
| `search` | The PCRE2 matcher behind every find and replace (ADR-005), result lines, the parity table. |
| `files` | Find in Files, Replace in Files and quick open's project walk on one walk builder, one `JobHandle` with a `CancelToken`, one open-documents index. |
| `session_store` | `session.json` and the backups on disk, the lock, crash safety (ADR-006). |
| `omarchy`, `desktop`, `sandbox`, `xdg`, `style_cache`, `settings_files`, `updates`, `recent` | The theme and font, the default editor, the Flatpak sandbox, Stet's folders, the generated style schemes, the settings files, the release check, reading the old recent-files list once. |

Infrastructure is synchronous. The app calls it from worker jobs; long runs (Find and Replace in Files, the project walk) stream results through a channel and stop on a `CancelToken`.

## The app

```
app/src/
  main.rs cli.rs application.rs activation.rs   the process, the command line, D-Bus activation (ADR-011)
  actions.rs                                     the registry turned into GActions, accelerators and gio::Menus
  preferences.rs monitor.rs config.rs logging.rs settings files, file monitors, environment, diagnostics
  theme/                                         the Omarchy theme as GtkSourceView schemes and CSS
  convert.rs                                     GTK's i32 offsets and sizes ↔ usize, once
  runtime/                                       worker jobs, Latest and SharedLatest tickets, Notify, Listeners,
                                                 Debounce, Flag, connect_once
  sys/                                           safe wrappers over the C calls the bindings lack (the only unsafe)
  editor/                                        one document: EditorPage (the tab's widget), Document and DocState,
                                                 loading, edits, marks, the page's search, the comparison view,
                                                 banners, column/ (the editor's view, StetView, and column mode)
  window/                                        the window: mod.rs builds it, dispatch.rs routes every action
    documents/ workspace/ text/ search/ tools/ chrome/   the window's features
    probe.rs                                     what the self-tests may read (feature `selftest`)
  selftest/                                      the `.stet-test` interpreter (feature `selftest`)
```

**Features own their state.** `Window` is a cloneable handle on `Inner`, which holds the layout and one state struct per feature (`FilesSearch`, `TabsState`, `CompareState`, …). A feature's module holds its state, wires its own signals in a `connect(window)` function, and runs its own commands.

**Commands.** Every user-visible command is an `ActionId` declared once in `domain/src/actions/registry.rs`. From the registry come the GActions and accelerators (`app/src/actions.rs`), the menus (`actions::Menus` → one `gio_menu` translation), the F1 palette and `keys.toml` names. When an action fires, `window/dispatch.rs` maps the `ActionId` to a feature's command enum in one `match` without a wildcard, and the feature matches its enum in full, so an action that isn't routed does not compile.

## Threads and the main loop

Nothing blocks the GTK thread (ADR-003). The pattern:

1. Take an owned snapshot on the GTK thread (`window::snapshot::PageSnapshot`: the text and the page's revision).
2. Run the work on GLib's thread pool (`runtime::worker::run`) or, for a small text, inline.
3. Back on the GTK thread, drop the result if the page was closed or edited meanwhile, or if a newer request superseded it (`runtime::Latest`; `runtime::SharedLatest` when the ticket must travel to a thread of its own, as quick open's ranking and walk do).
4. Apply edits to the buffer in one user action; more than 2,000 edits become one bulk edit (`text::BULK_EDIT_THRESHOLD`).

Waiting is event-driven: `EditorPage::loaded()`, `runtime::Notify`, signals. Polling loops and hand-made generation counters are not used.

## Opening and saving a file

`fs::read_file` (worker) reads, detects the encoding, decodes to LF and returns the text with a `DecodeReport`. `editor/load.rs` streams the text into the buffer in pieces, the first one a screenful, so the tab is usable at once. `DocState` keeps the report; the banners the tab shows come from `document::banners_for`. Saving encodes in the document's format and writes through `fs::save`, which keeps the file's mode and owner and never leaves a half-written file.

## Tests

| Layer | Where | Runs |
| --- | --- | --- |
| Domain | unit tests, `proptest` properties, `insta` snapshots next to the code | `cargo test`, no display |
| Infrastructure | `tempfile` folders, crash injection in the session store, the search parity table | `cargo test`, no display |
| App | headless unit tests (menus, conversions, runtime helpers) | `cargo test`, no display |
| UI | `.stet-test` scripts in `tests/selftest/`, interpreted by `stet --self-test` under GTK Broadway | `bash tools/ci.sh --ui` |

The self-test harness reaches the window only through `window/probe.rs`. Its commands are declared once per area (`commands!` tables in `app/src/selftest/`), and a test checks every script uses only declared commands. The harness is the Cargo feature `selftest`, on by default; released packages are built without it, and `tools/ci.sh` lints the app without it so production code can't depend on it.

## Rules the tools enforce

- `[workspace.lints]` in `Cargo.toml`: `unsafe_code` denied (allowed on `mod sys;` only), the lossy-cast lints, `too_many_lines`, `unwrap_used` (outside tests), `undocumented_unsafe_blocks`, and a chosen part of Clippy's pedantic group. `#[expect(…, reason = "…")]` marks the few exceptions; `allow_attributes` and `allow_attributes_without_reason` keep it that way. The two libraries also warn on `unreachable_pub`.
- `clippy.toml` disallows GTK's raw text positions (`TextBufferExt::iter_at_offset`, `iter_at_line`, `iter_at_line_offset`, and `TextIter::offset`, `line`, `line_offset` and their setters): offsets, lines and columns cross into `usize` only in `app/src/convert.rs`.
- `tools/ci.sh`: rustfmt, Clippy with `-D warnings` (with and without `selftest`), the tests, rustdoc with `-D warnings`, the build, and the check that the Flatpak's crate list matches `Cargo.lock`.

## Where new code goes

| You are adding… | Put it in |
| --- | --- |
| A rule, a calculation, a decision with no I/O | the domain context it belongs to, with tests |
| Reading or writing files, walking folders, a subprocess | infrastructure, with a headless test |
| A command | a row in `domain/src/actions/registry.rs`, a variant in the feature's command enum, an arm in `window/dispatch.rs` |
| A setting | a row in `domain/src/settings/mod.rs`'s `settings!` table |
| Something on screen | the window feature that owns it, or `editor/` if it belongs to one document |
| A GTK call the bindings lack | a safe wrapper in `app/src/sys/` |
| UI behaviour | a `.stet-test` script (and a probe method if the harness needs to read new state) |
