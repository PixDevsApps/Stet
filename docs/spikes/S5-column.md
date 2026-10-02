# S5: Column mode on GtkSourceView 5

> Recorded on 2026-09-30 under the working name Notes. Since [ADR-010](../DECISIONS.md#adr-010-accepted--stet-2026-09-30) the crate `notes-spikes` is `stet-spikes`, the view type `NotesS5ColumnView` is `StetS5ColumnView`, the clipboard MIME type `application/x-notes-column-block` is `application/x-stet-column-block`, and "Notes" in the text means Stet. The raw output and screenshots under `.local/` were deleted on 2026-09-30 to free disk space; re-run the spike to regenerate them. Decisions on its recommendations: [PLAN.md, 2026-09-30](../PLAN.md#name-repository-and-spike-changes-approved--2026-09-30).

**Date:** 2026-09-30 · **Milestone:** M0 · **Binary:** `spikes/src/bin/s5_column.rs`

**Verdict: FEASIBLE.** Column mode can be built on GtkSourceView 5 through gtk4-rs. Every mechanism the plan names worked headless on 10,000 lines:

- a `sourceview5::View` subclass paints the rectangle in `snapshot_layer`;
- a column insert is one undo step and takes about 29 ms against the 200 ms bar;
- typing, Backspace and paste are replicated through `insert-text`/`delete-range`.

M6 has to handle three things, covered under Findings:

1. GtkSourceView's whole-pixel tab stops. Without a fix, rectangles are jagged on lines with tabs, and tabs also misrender in ordinary editing.
2. Undo and redo of a 10k-line column edit take about 250 ms while a view is attached.
3. Undo granularity for typed text is one step per keystroke, unless the "grouped user action" technique measured here is used.

The fcitx5 part is **MANUAL** and not verified. It needs the live Hyprland session.

## Purpose

This spike checks plan §7 M0, row S5: a rectangle painted with `snapshot_layer`, plus fcitx5 typing, Backspace and paste replicated through `insert-text`/`delete-range`, with single-step undo over 10k lines. The pass bar is "Feasible". The M6 exit criterion used as the numeric bar: inserting at column 0 across 10k lines is one undo step and takes less than 200 ms. An S5 miss would reopen "column mode in 1.0, or GPUI".

## Method

**Command** (run 3 times, N = 1, 2, 3; each run takes about 2 minutes):

```
tools/headless.sh 15 cargo run --release -p notes-spikes --bin s5_column -- .local/s5-column-runN.md > .local/s5-run-N.jsonl
```

Each run writes one JSON line per measurement to stdout and a generated table to the `.md` argument. All three runs exited 0. The raw output is in `.local/s5-run-{1,2,3}.jsonl` and `.local/s5-column-run{1,2,3}.md` (git-ignored).

**The prototype, `ColumnView`**, is a GObject subclass of `sourceview5::View`. It implements `ViewImpl` and `TextViewImpl` and overrides two virtual functions:

- `snapshot_layer`: chains up to GtkSourceView first, then on `TextViewLayer::BelowText` paints the visible rows of the rectangle. It paints a 35%-alpha fill, or a 2 px caret bar for a zero-width rectangle.
- `paste_clipboard`: handles rectangular paste.

The rectangle is stored as anchor and cursor (line, visual column). The cursor line holds the real GTK caret.

**The test buffer** has 10,000 generated lines of seven shapes:

- empty;
- a leading tab;
- `short`;
- `abcdefgh\t= N;`, where a tab spans visual column 10;
- `héllo wörld — ünïcode N`;
- a 107-character line;
- indented code.

Tab width is 4, and the font is monospace.

**Correctness checks:**

- Every edit is compared with a pure-string model of the same column math (`locate`, `insert_in_line`, `delete_in_line`, `block_in_line`), using full text and a hash.
- The tab-aware column walker is cross-checked against `gtk_source_view_get_visual_column` at 250,000 positions.

**Timing:**

- Each timing is repeated 5 times per run. The tables show the three run medians, then the minimum and maximum over all 15 samples.
- The edit is timed synchronously.
- "Frame work" is the frame clock's `before-paint`→`after-paint` for the first frame after the edit.
- "Longest stall" is the largest gap between ticks of a 1 ms GLib timer over the 1.5 s after the edit. It includes that frame and any idle relayout.

**Pixel proof:**

- The view is rendered through `GtkWidgetPaintable` and `GskCairoRenderer`, and pixels are sampled on an empty line inside and outside the rectangle.
- This is done at the top of the buffer and again after scrolling to line 5000.
- Screenshots: `.local/s5-column-top.png` (GtkSourceView's own tab stops), `.local/s5-column-top-exact-tabs.png`, `.local/s5-column-scrolled.png`.

A watchdog thread exits with 124 after 420 s, and every wait is capped at 30 s. No run hit either limit.

## Environment

| | |
|---|---|
| CPU | AMD Ryzen 9 5900X 12-Core Processor, `nproc` 24 |
| RAM | 62.7 GiB (MemTotal 65,755,304 kB) |
| OS | Omarchy, Linux 7.2.5-3-omarchy |
| GTK / GtkSourceView / libadwaita | 4.22.4 / 5.20.0 / 1.9.3 (pkg-config) |
| Pango / GLib | 1.58.2 / 2.88.3 |
| Rust crates | gtk4 0.11.5, sourceview5 0.11.2 (feature `v5_18`), rustc 1.98.1, release profile (thin LTO) |
| Display | GTK Broadway (`tools/headless.sh 15`), no browser connected, view 900×661 px |
| Font | `monospace 14.667px`, which fontconfig maps to JetBrainsMono Nerd Font. Space advance 8.80 px (9011 Pango units). |

## Results

| # | Measure | Result | Pass bar | Verdict |
|---|---|---|---|---|
| 1 | `sourceview5::View` subclass with `ViewImpl` + `TextViewImpl::snapshot_layer` override | Builds and runs; `NotesS5ColumnView` is-a `GtkSourceView` | possible | **PASS** |
| 2 | `snapshot_layer` called on a realized view under Broadway | 6 layer calls and 99 rectangle rows in 3 frames (every run) | > 0 | **PASS** |
| 3 | Rectangle pixels, inside vs outside, at the top and scrolled to line 5000 | Inside `[184,212,245]` = fill blended over the outside `[255,255,255]`, exactly, at both positions in all runs | within 6/255 | **PASS** |
| 4 | Tab-aware visual-column walker vs `gtk_source_view_get_visual_column` | 0 mismatches in 250,000 positions | 0 | **PASS** |
| 5 | Column x on lines with tabs vs an empty line, **GtkSourceView's own tab stops** | 5 of 18 positions off by 1–34 px; the tab stop is 36 px, whole pixels, where 4 × 8.80 px = 35.2 px | ≤ 0.5 px | **FAIL** (finding 1) |
| 6 | Same, after `set_tabs` with 4 × space advance in Pango units (36044) | 0 of 18 off; pixel check OK | ≤ 0.5 px | **PASS** |
| 7 | Rectangle paint cost per frame; 10k-line rectangle, 35 visible rows painted | 181.8 / 190.1 / 205.2 µs (range 155–259 µs) | — | not judged |
| 8 | **Insert `// ` at column 0 across 10k lines, one user action, view attached** | **28.97 / 29.29 / 28.65 ms** (range 28.2–30.2); text matches the model | < 200 ms | **PASS** |
| 9 | Same at column 10, with virtual-space padding and tab snapping | 34.40 / 34.56 / 34.45 ms (range 33.7–36.3); 8,571 lines land on column 10; the 1,429 lines with a tab across it snap to the tab start | < 200 ms | **PASS** |
| 10 | Same at column 0, buffer only (no view) | 16.31 / 16.33 / 16.77 ms (range 16.2–17.5) | — | reference |
| 11 | **One `undo()` restores the original, `redo()` re-applies** (rows 8–10) | Hash equal (`a5b3ca9eb2750fce`) after 1 undo; `can_undo` is false after it; redo equals the insert, in 15/15 samples per case | 1 step | **PASS** |
| 12 | Undo / redo time, view attached (rows 8–9) | Median of run medians: undo 249.8 ms (col 0) / 249.6 ms (col 10), range 235–271; redo 238.4 / 246.2 ms, range 235–271. Buffer only: undo 34.1 ms, redo 33.6 ms | — (no bar) | not judged (finding 2) |
| 13 | UI cost after a 10k-line column insert | Frame work 12.3 / 12.4 / 11.7 ms (range 11.1–12.5); longest stall 27.8 / 27.8 / 26.7 ms (range 12.4–29.1) | — | not judged |
| 14 | Alternative: one delete + one insert of the whole line range | Edit 6.8 ms (range 6.6–10.4), undo 4.5 ms, redo 4.2 ms. A mark on line 5000 moves to line 0 and stays there after undo. | — | not judged (finding 2) |
| 15 | **Typed text replicated via `insert-text`**: 5 × `insert_interactive_at_cursor` (the call GtkTextView's IM commit makes), rectangle on all 10k lines, primary line in virtual space | All 10k lines match the model; the rectangle advanced to column 15 | works | **PASS** |
| 16 | Per-keystroke cost, 10k-line caret | Edit 34.35 / 34.70 / 34.08 ms (range 32.0–36.5); frame work 10.2 / 11.3 / 10.5 ms; longest stall 10.9 / 11.6 / 11.1 ms (run medians) | — | not judged |
| 17 | Undo steps for 5 replicated keystrokes | 5 steps (one per keystroke) back to the original; redo 5. Typing on one line without a rectangle gives 1 step. | 1 step | **FAIL** as-is (finding 3) |
| 18 | **Alternative: grouped user action**, held open from the first replicated keystroke until column mode ends | 5 keys + Backspace = **1 undo step**, redo 1; correct in all runs | 1 step | **PASS**, with conditions (finding 3) |
| 19 | Key-binding path: the view's `insert-at-cursor` signal | Replicated; 34.7–36.4 ms (one sample per run) | works | **PASS** |
| 20 | **Backspace replicated via `delete-range`** (`buffer.backspace`, then the view's `backspace` signal) | Both replicated correctly; 44.1–47.1 ms (6 samples); all edits undo back to the original | works | **PASS** |
| 21 | Delete a 5-column block across 10k lines, one user action | 45.47 / 44.61 / 44.74 ms (range 44.3–47.1); matches the model; 1 undo step | < 200 ms | **PASS** |
| 22 | Rectangular clipboard: union `ContentProvider` of `application/x-notes-column-block` + `text/plain;charset=utf-8` | Both offered; `read_text` returns the joined block | works | **PASS** |
| 23 | Paste a 10k-row block at column 40 through the `paste_clipboard` override (async read included) | 48.65 / 49.57 / 48.27 ms (range 46.1–60.5); matches the model; 1 undo step; plain-text clipboards fall back to GTK's paste | 1 step | **PASS** |
| 24 | GLib/GTK warnings and criticals | 0 in every run | 0 | **PASS** |
| 25 | fcitx5 preedit and commit in column mode | Not run | works | **MANUAL** |

Every verdict above was identical in all three runs.

## How it works (reference for M6)

- **Subclassing.** `sourceview5::subclass::view::ViewImpl` exists. It has `line_mark_activated`, `show_completion`, `move_lines`, `move_words` and `push_snippet`. It requires `TextViewImpl`, whose `snapshot_layer`, `paste_clipboard`, `copy_clipboard`, `backspace` and similar methods can all be overridden.
  - Chain up first in `snapshot_layer`, because GtkSourceView paints in these layers too.
  - The layer snapshot is in **buffer coordinates**. Painting at `iter_location`/`line_yrange` values was pixel-correct after scrolling to line 5000, with no `buffer_to_window_coords` conversion. That call was only used to find the pixels in the widget-space render.
- **Painting.** For each visible line (`line_at_y` over `visible_rect`), paint from x(left column) to x(right column).
  - For a column inside the text, x comes from `iter_location` of the character at that column.
  - For virtual space past the line end, x is `iter_location(line end) + missing columns × space advance`.
  - Only visible rows are painted: about 0.19 ms per frame, even for a 10k-line rectangle.
- **Replication.** gtk4-rs's `connect_insert_text` connects *before* the default handler, and its iter is a copy.
  - In that before handler, record (line, offset, visual column, text).
  - After the default handler, run `connect_local("insert-text", true, …)` to insert the same text at the rectangle's column on every other line, and to pad the primary line when its caret sat in virtual space. Take the text from the before handler, because the raw signal string is not guaranteed to be NUL-terminated.
  - `delete-range` works the same way. The after handler's iters are already collapsed, so the deleted column range must be captured before.
  - A `replicating` guard stops the re-entry.
  - The replicated edits run inside the user action that GTK's own call opened (`insert_interactive_at_cursor` / `backspace`), so they share its undo step.
- **Undo/redo guard.** Handlers connected normally to the buffer's `undo`/`redo` signals run before GTK's class handler. This is verified: 50,001 of undo's own edits hit the guard. They switch replication off and leave column mode. Without them, undoing a primary-line edit would be re-replicated.
- **Clipboard.** Copy sets a union provider carrying the same bytes, rows joined by `\n`, under the custom MIME type and under `text/plain`.
  - Paste overrides `paste_clipboard`. If `formats()` contains the custom type, it reads it asynchronously (`read_future` + `read_bytes_future`) and inserts row *i* at (top + *i*, column), padding with spaces and appending newlines past the end of the buffer, in one user action. Otherwise it chains to GTK.
  - GDK also advertises the GTypes it can deserialize the text into (`gchararray`, `GtkTextBuffer`).
  - Other applications get plain text. Pasting across processes on Wayland was not tested, because the Broadway clipboard is process-local.

## Findings and risks

1. **GtkSourceView's tab stops are whole pixels, which misaligns tabs whenever the font advance is fractional.** This affects all editing, not only column mode.
   - `view.tabs()` reads back a 36 px stop (`positions_in_pixels`), where 4 columns are 35.2 px.
   - After `abcdefgh`, which ends at 70.4 px, the next stop is 72 px, so that tab is drawn less than 2 px wide instead of 4 columns (visible in `s5-column-top.png`).
   - Rectangle edges on those lines were 1–34 px off.
   - `set_tabs` with the stop in Pango units (4 × 9011 = 36044) fixed every position.
   - GtkSourceView **resets its own tabs** when `tab-width` changes (measured), so the fix must be re-applied on `notify::tab-width`. Font changes were not tested and probably need the same re-application.
2. **Undo/redo of a 10k-line column edit takes about 250 ms with a view attached**, against about 34 ms without one.
   - GTK's undo emitted 20,000 `mark-set` signals for the 10,000 sub-edits (zero during the edit itself). The view reacts to each cursor/selection move.
   - Rewriting the line range with one delete + one insert costs 6.8 ms to edit and 4.5 ms to undo. But marks inside the range, such as bookmarks, collapse to its start and are not restored by undo.
   - M6 options:
     - use per-line edits up to a line threshold and a range rewrite above it, saving and restoring bookmarks by line;
     - or accept the cost. A 10k-line column edit is rare, and the insert itself is fast.
3. **Typed text in column mode is one undo step per keystroke.** GTK merges plain typing into one step but does not merge multi-line groups.
   - Holding one `begin_user_action` open across a typing burst gives exactly one step, including Backspace.
   - However, while the group is open, `can_undo` is false even when earlier history exists, and `undo()` does nothing. This is measured.
   - M6 must therefore close the group before anything else acts. Triggers:
     - a capture-phase key controller for any Ctrl/Alt/Super chord, navigation key or Escape;
     - focus-out;
     - a click;
     - opening a menu or the palette;
     - leaving column mode;
     - a short idle timeout.
   - Closing, then undoing, removed exactly the burst. The fallback, one step per keystroke, is functional and could ship if the grouping proves fragile.
4. **Per-keystroke cost grows with the number of rectangle lines.** At 10k lines a keystroke costs about 34 ms of editing plus about 11 ms of frame work, roughly 3.4 µs per line (derived from the 10k-line figure, not measured at other sizes). Typical rectangles of tens to hundreds of lines should cost well under a frame, but this was not measured on a real compositor.
5. **Tabs across the rectangle's column.** The prototype snaps to the start of the tab. This is consistent everywhere: edit, paint, copy and delete. But typing across a tab stop produces `ab\tcde`: characters go before the tab until the typed text reaches the stop, then after it. M6 must choose, and pin in the column-math proptests, between snapping and splitting the tab into spaces.
6. **Not prototyped:**
   - typing over a non-empty block (needs a block delete plus the insert in the same after handler);
   - Enter in column mode;
   - overwrite mode;
   - the Delete key (the before handler accepts `from == left`, but this is untested);
   - Alt+drag and Alt+Shift+arrow selection. GtkSourceView binds Alt+Shift+arrows to `move-viewport` (plan §5), so the binding must be taken over.
   - Also, multi-line inserts and edits away from the caret leave column mode.
7. **Visual columns count one cell per character**, as `gtk_source_view_get_visual_column` does. The em dash in the test lines rendered as one cell in this font. East Asian wide characters were not tested; they would make the rectangle follow glyph x positions, giving jagged edges.
8. **Broadway limits the timing evidence.**
   - With no browser connected, Broadway starts frames up to 0.76–1.72 s after they are requested. So "frame work" measures only the work inside the frame, and keystroke-to-paint was not measured.
   - Painting went through Broadway and a Cairo readback, not Wayland/GL.

### MANUAL checks for the live session (not run)

- Type Latin text through fcitx5 in a 3–5 line column caret. Commit should replicate on every line, and preedit is expected to show only at the primary caret.
- CJK preedit, then commit: the commit replicates, and a candidate window follows the primary caret.
- Dead keys and compose sequences.
- Ctrl+Z straight after an IME burst, with grouping on.
- Copy a rectangle in Notes and paste it into another app, which should get plain text. Then paste it into a second Notes window or instance, which should get a rectangle.
- Keystroke-to-paint on Hyprland with a 1k-line and a 10k-line column caret.

## Effort estimate for M6

About **10–11 focused days**, inside the plan's 8–12 days:

| Work | Days |
|---|---|
| Production `ColumnView`: rectangle model, painting, virtual-space caret, exact tab stops (re-applied on tab-width and font changes) | 1.5 |
| Selection: Alt+drag (x→column with virtual space), Alt+Shift+arrows (take over `move-viewport`), clearing on caret moves | 2 |
| Editing: typing and Backspace/Delete replication, typing over a block, Enter, overwrite mode, grouped undo with close triggers | 2 |
| Clipboard: copy, cut and paste of rectangles, plain-text fallback, paste past the end of the buffer | 1 |
| Column Editor (Alt+C: text or numbers, start, step, repeat, leading zeros, base) | 1.5 |
| Large rectangles: range-rewrite threshold plus bookmark save and restore | 1 |
| Column-math proptests, headless tests, live fcitx5/Hyprland validation | 1.5 |

The column-math functions in the spike (`locate`, `insert_in_line`, `delete_in_line`, `block_in_line`) are pure. They can move to `domain/ops/column.rs` as the proptest target.

## Recommendations for the plan

1. **S5 go.** Keep column mode in 1.0 on GtkSourceView. Nothing here argues for GPUI.
2. **Move the exact-tab-stop fix to M1** (editor setup), because it corrects tab rendering for every user whose font advance is fractional. Add a headless test that compares `iter_location` x after a tab with the character grid.
3. **Add to the M6 exit criteria:**
   - "undo and redo of a 10k-line column edit < 200 ms". Per-line edits currently take about 250 ms, so this needs the range-rewrite threshold, or a relaxed bar if bookmark preservation proves too costly.
   - "a column typing burst undoes in one step". Keep one step per keystroke as the documented fallback.
4. **Pin the column semantics in the M6 proptests:** tab snapping versus splitting, virtual-space padding, and the rule that a column is one character except for tabs.
5. **Measure keystroke-to-paint in column mode on Hyprland.** It is not measurable under Broadway.

## Addendum: live re-measure on Hyprland (2026-09-30)

Later the same day `STET_SPIKE_ALLOW_LIVE=1 target/release/s5_column <out.md>` ran once in the user's live Hyprland session (144 Hz, release build). The generated table keeps the Broadway wording; those rows are Wayland in this run. Details are in [S4-live.md](S4-live.md#live-re-measure-of-s5-column-mode-10k-lines).

- The results match the headless run. Inserting across 10k lines takes 28.3 ms (column 0) and 33.0 ms (column 10). A replicated keystroke takes 33.3 ms plus 1.6 ms of frame work. Grouped typing is one undo step, and there were 0 warnings.
- Undoing per-line edits over 10k lines takes 238 ms (redo 237 ms), still over the 200 ms bar. A range rewrite undoes in 4.67 ms.
- Tab stops are misaligned as GtkSourceView sets them (5 of 18 positions off) and aligned in Pango units.
- The fcitx5 column-caret check was not completed in the interactive session and moves to M6's acceptance (proposed).
