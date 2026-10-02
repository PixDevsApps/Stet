# S1: Performance, with S6: Startup

> Recorded on 2026-09-30 under the working name Notes. Since [ADR-010](../DECISIONS.md#adr-010-accepted--stet-2026-09-30) the crate `notes-spikes` is `stet-spikes`, the app crate and binary `notes` are `stet`, and the `NOTES_*` environment variables are `STET_*`. The raw per-run JSON and logs under `.local/s1/` and the generated fixtures under `target/fixtures/` were deleted on 2026-09-30 to free disk space; regenerate the fixtures with `python3 tools/gen-fixtures.py` and re-run the spike to regenerate the output. Decisions on its recommendations: [PLAN.md, 2026-09-30](../PLAN.md#name-repository-and-spike-changes-approved--2026-09-30).

**Date:** 2026-09-30 · **Milestone:** M0 · **Binary:** `spikes/src/bin/s1_perf.rs` · **Runs:** headless under GTK Broadway, display `:11`

## Verdicts

| Item | Pass bar | Result (median of 3 runs unless noted) | Verdict |
|---|---|---|---|
| 100 MB / 1M lines: first screen | ≤ 1.5 s | 126 ms with timer-paced 4 MB chunks, no highlighting. 147 ms with Rust highlighting. A single `set_text` gives 1.01 s, but the main thread is blocked for 0.91 s. | **PASS** |
| 100 MB: RAM | ≤ 3× file size | No highlighting: 2.67× once settled, 3.11× peak while loading. Rust highlighting: 7.04×. | **PASS** with highlighting off, as large mode specifies; peak is marginal. **FAIL** with highlighting. |
| Typing in 10 MB Rust, highlighting finished | < 16 ms keystroke-to-paint | End of file: 7.7 ms median, 8.3 ms p95. Middle: 8.9 / 9.6 ms. End of line 20: 5.2 / 6.2 ms. | **PASS** |
| Typing at offset 0 of the same file (before `#![allow(...)]`) | < 16 ms | 29.5 ms median, 31.2 ms p95 | **FAIL** |
| Typing in 10 MB Rust in the ~7 s after loading, while highlighting still runs | < 16 ms | 80–104 ms median, 1.45–1.79 s p95. No frame was painted for up to 2.05 s. | **FAIL** |
| Typing in 10 MB, no highlighting (baseline) | < 16 ms | 2.5–6.4 ms median at every position, p95 ≤ 7.1 ms, both after settling and right after loading | **PASS** |
| 10 MB single-line JSON through the long-line path (worker pretty-print) | No freeze > 2 s | Longest main-loop block 64–67 ms. First screen 249 ms (wrap off) and 263 ms (wrap on). | **PASS** |
| 10 MB single-line JSON inserted raw, for comparison | No freeze > 2 s | Main thread blocked ≥ 89 s with wrap off and with wrap on, in 3 of 3 runs each. Killed at the deadline. | **FAIL** |
| Snapshot `buffer.text()` | Measured; ADR-003 is revisited if > 100 ms at 50 MB | 10 MB: 8.1 ms. 50 MB: 27.7 ms. | **PASS**, measured |
| S6 cold start | ≤ 400 ms | 56 ms from `main` to first frame, 109 ms from `exec`. With a 10 MB file: 188 ms and 238 ms. Warm page cache, 5 runs. | **PASS** (warm cache only) |
| S6 second launch forwarding | ≤ 100 ms | 61 ms wall, 5 runs. I did not check that the tab opened. | **PASS** (partly verified) |
| S6 `--wait` through D-Bus activation | Works | Not implemented in M0 | **NOT TESTED** |

**Summary for the go/no-go:**
- GtkSourceView handles the 100 MB plain-text case: first screen in about 0.13 s and 2.67× RAM once settled, **provided** chunks are inserted from a timer rather than an idle.
- Syntax highlighting is the weak spot:
  - it costs about 0.4–0.5 KiB of RAM per line (7× file size at 1M lines);
  - while it runs its initial pass over a 10 MB file, repaints can be held off for up to 2 s;
  - one edit position cost 27 ms per frame.
- The raw long-line path freezes for over 89 s. The plan's formatted path works.

These are Broadway numbers. Real-Hyprland keystroke-to-paint must be re-measured by hand (see Caveats).

## Purpose

This spike measures plan §7 M0 row S1 and the startup part of S6 against the pass bars in [TESTING.md](../TESTING.md#benchmarks-and-performance-targets). An S1 miss triggers an ADR for a large-file viewer.

## Environment

| Item | Value |
|---|---|
| CPU | AMD Ryzen 9 5900X 12-Core Processor (`/proc/cpuinfo`), `nproc` 24, governor `performance` |
| RAM | 62 GiB (MemTotal 65,755,304 kB) |
| Kernel | 7.2.5-3-omarchy |
| Libraries (pkg-config) | gtk4 4.22.4, gtksourceview-5 5.20.0, libadwaita-1 1.9.3, glib-2.0 2.88.3, pango 1.58.2, cairo 1.18.4. The runtime reported the same gtk and gtksourceview versions. |
| Crates | gtk4 0.11.5, sourceview5 0.11.2, libadwaita 0.9.2. Built with rustc 1.98.1, release profile (thin LTO, 1 codegen unit). |
| Font | `monospace` resolves to JetBrainsMono Nerd Font |
| Load | 1-minute load average 1.0–3.1 at case start. Other agents were compiling and running spikes on the same machine. |
| Fixtures | `python3 tools/gen-fixtures.py`: `big-1m-lines.txt` (98,124,125 B, 1,000,000 lines of Rust-like ASCII), `medium-10mb.rs` (10,487,175 B, 191,376 lines), `single-line-10mb.json` (10,485,785 B, no newline) |

## Method

### Commands

All from the project root, on 2026-09-30 between 21:13 and 22:03 CEST.

```
cargo build --release -p notes-spikes --bin s1_perf
NOTES_HEADLESS_TIMEOUT=3000 tools/headless.sh 11 cargo run -q --release -p notes-spikes --bin s1_perf -- open
NOTES_HEADLESS_TIMEOUT=3000 tools/headless.sh 11 cargo run -q --release -p notes-spikes --bin s1_perf -- open-timer-chunks-plain open-timer-chunks-rust
NOTES_HEADLESS_TIMEOUT=3000 tools/headless.sh 11 cargo run -q --release -p notes-spikes --bin s1_perf -- long-line
NOTES_HEADLESS_TIMEOUT=1500 tools/headless.sh 11 cargo run -q --release -p notes-spikes --bin s1_perf -- typing snapshot   # snapshot data used
NOTES_HEADLESS_TIMEOUT=1500 tools/headless.sh 11 cargo run -q --release -p notes-spikes --bin s1_perf -- typing            # final typing data
NOTES_HEADLESS_TIMEOUT=100 tools/headless.sh 11 target/release/s1_perf --case long-line-raw-nowrap                        # one diagnostic run
```

`-- all` runs every case. The raw per-run JSON is in `.local/s1/results-<unix-time>.json` (git-ignored):

| Data | File |
|---|---|
| open, idle-paced and `set_text` | `1790795624` |
| open, timer-paced | `1790796565` |
| long-line | `1790796857` |
| snapshot | `1790797595` |
| typing | `1790798154` |

Child stderr logs are next to them. The open-idle and `set_text` cases ran on an earlier build of the same file. Their code path did not change afterwards, apart from a refactor that turned `chunked: bool` into `Load`.

### Harness

The parent process runs each case 3 times, each in a fresh child process. It parses the JSON lines the child prints and kills the child at a per-case deadline: 240 s for open, 180 s for typing and formatted long-line, 90 s for raw long-line, 120 s for snapshot.

**Window.** Each child builds a plain `gtk::Window` of 1000×700 after `libadwaita::init()`. The window holds a `ScrolledWindow` with a `sourceview5::View` set up as in the app:
- monospace, line numbers, current-line highlight;
- the Adwaita style scheme;
- focus on the view.

**Painted frame.** A painted frame is an `after-paint` on the window's `GdkFrameClock`. The "first frame after X" is the first frame whose `before-paint` came after X. Frame work is `before-paint` to `after-paint`.

**Main-loop block.** A 5 ms GLib timer records the largest gap between its runs; synchronous work done by the measuring code counts too. A watchdog thread prints a block still in progress every second, so a freeze is still measured when the parent has to kill the child.

**Settled.** The main thread's CPU time, read from `/proc/thread-self/schedstat`, stays under 10% over a sliding 250 ms window. This is when GtkTextView line validation and GtkSourceView background highlighting have finished. The cap is 60 s.

**RSS.** `VmRSS` and `VmHWM` from `/proc/self/status`. The "RAM ×" ratios are (RSS − baseline) ÷ file bytes. The baseline is the RSS with the empty window shown: 90.3–92.8 MiB for the open cases. The long-line baseline (100.4–100.6 MiB) already includes the 10 MB source string.

**Broadway web client.** Without a connected browser, GDK Broadway rate-limits every surface to one frame per second. The snapshot case measured 1.0 Hz continuous frame rate in all 3 runs (`1790795060`). The parent therefore connects to broadwayd's WebSocket (`/socket`, protocol `broadway`) and behaves like `broadway.js`:
- it parses the op stream;
- it answers `ROUNDTRIP` with `ROUNDTRIP_NOTIFY`;
- it sends `CONFIGURE_NOTIFY` for new and resized surfaces, and a 1920×1080 screen size;
- it draws nothing.

With the client, frames are no longer paced: 20,979 Hz median continuous rate on an idle window (range 20,361–21,259).

**Cursor blink** is turned off (`gtk-cursor-blink=false`). Unpaced, the blink animation repainted continuously and kept the main thread busy for the 10 s blink timeout. In a first typing run with blink on, it pushed the "settled" time for the plain 10 MB file from about 6 s to 9.9 s.

### Cases

**a) Open 100 MB.** Read with `std::fs::read` and `String::from_utf8` on the main thread, then insert in one of three ways:

- **Idle chunks.** Chunks of about 4 MiB cut at line ends. Insert the first, wait for its frame, then one chunk per default-priority idle (`G_PRIORITY_DEFAULT_IDLE`). This is the plan's literal description.
- **Timer chunks.** Same chunks, one per 16 ms default-priority timeout.
- **Single `set_text`.**

Each insert is inside `begin/end_irreversible_action`, the view is non-editable, and the source string is dropped after inserting.

Highlighting is either off, or on with the `rust` language set before inserting (the fixture is Rust-like despite `.txt`). One more variant sets the language after the last chunk, which is the plan's flow.

"First screen" is measured from before the read to the first frame after the first chunk, so it includes the read and the decode.

**b) Typing.** `medium-10mb.rs` is loaded with `set_text`, with and without Rust highlighting. Keystrokes are mimicked the way GtkTextView commits text: `begin_user_action`, `insert_interactive_at_cursor("x")`, `end_user_action`, `scroll_mark_onscreen`. They are timers scheduled every 100 ms, 50 per series, so 150 samples over 3 runs.

The latency is the keystroke dispatch to the end of the first frame that began after it. Input delay is the dispatch time minus the scheduled time.

Series, after settling:
- end of file;
- end of the middle line;
- end of line 20;
- offset 0.

Series right after a fresh `set_text`, while background work still runs:
- end of line 20;
- offset 0.

**c) Long line.** `single-line-10mb.json` is used in two ways:
- **Raw.** Inserted into a view with wrap off (`WrapMode::None`) or on (`WrapMode::WordChar`), highlighting off.
- **Formatted.** Pretty-printed on a worker thread with `serde_json::from_str::<Value>` and `to_string_pretty`, while the main loop is polled. It is then inserted with timer chunks, with the `json` language on.

**d) Snapshot.** `buffer.text(start, end, true)` runs 5 times per run on:
- a 10 MB buffer (`medium-10mb.rs`);
- a 50 MB buffer (the first 50 MiB of the big fixture, cut at a line end: 52,428,845 B).

Each run also times the copy into a Rust `String`, and checks that the snapshot equals the inserted text.

**e) S6.** `cargo build --release -p notes`, then 5 runs of each:

```
tools/headless.sh 11 env NOTES_EXIT_AFTER_MS=800 target/release/notes
tools/headless.sh 11 env NOTES_EXIT_AFTER_MS=800 target/release/notes target/fixtures/medium-10mb.rs
tools/headless.sh 11 env NOTES_EXIT_AFTER_MS=800 bash -c 'echo "exec $(date +%s.%N)"; exec target/release/notes "$@"' _ [file]
```

The app logs `first-frame <ms since the top of main>`. The third form stamps the time just before `exec`; the elapsed time comes from comparing that stamp with the log line's UTC timestamp.

Second-launch forwarding was measured in one headless session:
1. Start a primary with `NOTES_EXIT_AFTER_MS=6000`.
2. Wait for its `first-frame`.
3. Time 5 runs of `NOTES_EXIT_AFTER_MS=1500 target/release/notes Cargo.toml` with `date +%s%N`.

All runs exited 0.

## Results

### a) Opening the 100 MB file

The read took 31 ms (30.1–35.4 ms), from the page cache. UTF-8 validation took 3.3–5.9 ms. Every run ended with 98,124,125 characters and 1,000,001 buffer lines; the last line is the empty one after the final newline.

| Case | First screen | All inserted and painted | Longest block while loading | Settled after | RSS at end | RAM × (end / peak) |
|---|---|---|---|---|---|---|
| Timer chunks, plain | 125.6 ms (125.2–127.4) | 1,036.5 ms (1,029.4–1,036.6) | 62.1 ms (61.6–62.8) | 39.5 s (39.4–39.7) | 340.4 MiB | 2.67 / 3.11 |
| Timer chunks, Rust | 146.9 ms (146.8–146.9) | 1,069.7 ms (1,062.4–1,070.2) | 66.7 ms (66.6–66.7) | 51.3 s (51.2–51.4) | 751.5 MiB | 7.04 / 7.04 |
| Idle chunks, plain | 127.1 ms (123.2–132.3) | 40,434 ms (40,314–41,016) | 80.1 ms (79.4–80.6) | 62.5 s (62.4–63.2) | 339.4 MiB | 2.66 / 3.62 |
| Idle chunks, Rust set before inserting | 144.6 ms (144.2–146.4) | 41,114 ms (40,651–41,189) | 87.7 ms (87.4–89.1) | 74.8 s (74.2–74.9) | 752.0 MiB | 7.05 / 7.05 |
| Idle chunks, Rust set after loading | 122.0 ms (121.6–124.0) | 40,483 ms (40,171–40,735) | 81.1 ms (80.8–81.8) | 74.2 s (74.0–75.2) | 751.3 MiB | 7.06 / 7.06 |
| `set_text`, plain | 1,010.3 ms (1,006.1–1,025.8) | same | 909.9 ms (906.1–926.5) | 39.3 s (39.2–39.8) | 340.0 MiB | 2.67 / 4.08 |
| `set_text`, Rust | 1,048.2 ms (1,031.9–1,049.4) | same | 925.5 ms (912.7–927.8) | 51.5 s (51.4–51.5) | 751.4 MiB | 7.04 / 7.04 |

**Chunk inserts and the first frame:**
- Each 4 MiB `insert` took 38–41 ms.
- The first frame after the first chunk took 50–57 ms of frame work.
- `set_text` of 100 MB blocked for 0.91–0.93 s.

**After loading:**
- The main thread stayed busy until "settled": 39 s plain and 51 s with Rust, counted from the start of the load. The work is GtkTextView validating every line's height, and GtkSourceView highlighting the whole buffer.
- It ran in slices. The longest block while settling was 7.6–12.0 ms plain and 47–50 ms with Rust.

**Language set after loading:**
- `set_language` returned in 7.8 ms and the next frame followed 69.9 ms later.
- The longest block was 48.5 ms.
- It did not save memory in the end (7.06×).

**Idle-paced chunks.** They took about 40 s to load because a default-priority idle only runs when GtkTextView's validation idles (which run at a higher priority) are idle. Validation of each chunk therefore finishes before the next chunk is inserted.

**Highlighting memory:**
- Rust highlighting over 1M lines grew RSS by 411 MiB compared with plain, about 431 B per line.
- Before highlighting ran, RSS was about 295 MiB once all chunks were in. It reached 751 MiB after highlighting.

### b) Typing latency, 10 MB file

Insert-to-paint in ms, as median / p95 / max over 150 keystrokes. "Frame" is the median frame work. "Longest frame gap" is the largest gap between consecutive painted frames during the series; the median over 3 runs is given, with the range. The keystroke interval is 100 ms, so a gap of about 100 ms is normal.

| Series | Rust highlighting | Frame | Longest frame gap | Plain | Frame |
|---|---|---|---|---|---|
| End of file, after settling | 7.70 / 8.31 / 8.83 | 4.65 | 101.6 | 4.84 / 5.23 / 5.38 | 4.70 |
| End of middle line, after settling | 8.86 / 9.60 / 11.39 | 6.28 | 101.1 | 6.38 / 7.13 / 8.22 | 6.15 |
| End of line 20, after settling | 5.16 / 6.17 / 9.12 | 3.28 | 101.3 | 3.57 / 4.05 / 4.69 | 3.37 |
| Offset 0, after settling | 29.50 / 31.15 / 32.11 | 27.40 | 102.2 | 2.91 / 3.48 / 3.58 | 2.77 |
| End of line 20, right after loading | 80.20 / 1,453.69 / 1,762.55 | 6.08 | 1,757.7 (1,126.9–1,762.6) | 3.28 / 3.43 / 3.74 | 3.16 |
| Offset 0, right after loading | 104.48 / 1,794.57 / 2,046.27 | 26.40 | 2,028.3 (1,890.2–2,046.3) | 2.52 / 2.80 / 4.44 | 2.43 |

**Load and settle times:**
- Loading to the first frame took 163.5 ms with Rust (159.3–164.2) and 137.1 ms plain (135.6–140.1).
- Settling took 7.40 s with Rust (7.34–7.69) and 6.04 s plain (5.99–6.07).

**Input delay** (dispatch minus schedule):
- After settling: median 0.08 ms, max ≤ 0.17 ms.
- Right after loading, with Rust: median 1.3–2.1 ms, p95 about 90 ms, max 124 ms.
- The longest main-loop block right after loading with Rust was 121–141 ms. That is far shorter than the 1.1–2.0 s frame gaps, so the main loop kept dispatching while no frame was painted.

My reading is that GtkSourceView's initial highlighting work runs at a priority above GDK's redraw priority. I did not verify that in the GtkSourceView source.

In the offset-0 series every frame took about 27 ms. The typed characters go before the crate attribute `#![allow(dead_code, unused)]`.

### c) 10 MB single-line JSON

**Raw insert.** It never produced a frame.

| Case | Outcome (3 runs each) |
|---|---|
| Raw, wrap off | Killed at the 90 s deadline. Main thread blocked 89.03 s at kill time in every run. |
| Raw, wrap on (`WORD_CHAR`) | Killed at the 90 s deadline. Blocked 89.03 s at kill time in every run. |

In one diagnostic run (wrap off), the `insert` call itself took 71.3 ms. The block started at the next step, `place_cursor`, and lasted until the kill at 100 s (≥ 98.0 s). Without the cursor move, the first layout of the visible line would presumably block the same way; I did not measure that.

**Formatted path** (worker pretty-print, then timer chunks with JSON highlighting). The output was 17,126,204 B and 836,232 lines, in 5 chunks.

| Measure | Wrap off | Wrap on |
|---|---|---|
| Parse (`from_str::<Value>`) on worker | 96.7 ms (96.0–98.5) | 103.2 ms (101.4–104.5) |
| Pretty print on worker | 32.8 ms (32.0–34.4) | 34.7 ms (33.8–36.9) |
| Format wall time seen by main thread | 150.9 ms (150.9–156.8) | 161.0 ms (160.9–167.2) |
| Longest main-loop block while formatting | 5.1 ms | 5.1 ms |
| First screen (from worker start) | 249.1 ms (248.0–255.1) | 262.8 ms (257.7–270.6) |
| All inserted and painted | 467.4 ms (463.2–472.2) | 486.3 ms (473.7–495.4) |
| Longest main-loop block, whole run | 64.0 ms (63.9–69.5) | 66.8 ms (64.6–67.0) |
| Settled after | 25.5 s (25.4–25.6) | 27.3 s (27.3–27.3) |
| RSS at end | 645.5 MiB | 645.3 MiB |
| RAM × of the 10 MB source | 54.5 | 54.5 |

RSS was about 203 MiB right after inserting, before background highlighting. It then grew by about 443 MiB, or about 555 B per line, including validation.

### d) Snapshot cost

15 samples each (5 per run × 3 runs).

| Buffer | `buffer.text(start, end, true)` | Copy to `String` | `set_text` of the same text (3 runs) |
|---|---|---|---|
| 10 MB (10,487,175 B) | median 8.13 ms (7.83–11.75) | 0.46 ms (0.31–1.92) | 108.2 ms (106.5–108.7) |
| 50 MB (52,428,845 B) | median 27.72 ms (27.28–31.58) | 4.58 ms (4.46–5.51) | 484.2 ms (483.7–495.9) |

### e) S6 startup (5 runs each, release build)

| Measure | Runs (ms) | Median |
|---|---|---|
| `first-frame`, no file (exact command) | 57.4, 57.3, 55.7, 56.4, 55.8 | **56.4 ms** |
| `first-frame`, 10 MB `.rs` file | 188.2, 186.9, 192.8, 190.8, 185.1 | **188.2 ms** |
| `exec` to first frame, no file | 109.1, 110.3, 108.6, 110.4, 108.8 | **109.1 ms** (`main`-based: 56.1) |
| `exec` to first frame, 10 MB file | 236.3, 238.2, 238.3, 242.6, 247.6 | **238.3 ms** (`main`-based: 187.4) |
| Second launch forwarded to a running primary (wall, includes process start) | 59.6, 60.6, 61.5, 60.9, 62.1 | **60.9 ms** |

About 52 ms passes between `exec` and `main`: loading the dynamic libraries and running their constructors.

With the 10 MB file, the app's M0 `open_file` reads and runs `set_text` on the main thread with Rust highlighting.

**Second launch:**
- The secondaries printed nothing and exited 0, so none became a primary.
- The primary logs no open events, so I did not check that the tab opened.
- `--wait` and D-Bus activation do not exist yet (M2), so that part of S6 was not tested.

## Caveats

- **Broadway, not Wayland/GL.**
  - Frames go through GSK's Broadway renderer: render nodes are serialized, and text is uploaded as textures. Frame work on Hyprland with the GL renderer will differ.
  - The web client answers roundtrips at once, so frames are **unpaced**. On Hyprland, a frame waits for the compositor's frame callback, which adds up to one refresh interval (16.7 ms at 60 Hz, 6.9 ms at 144 Hz) plus the compositor's own latency.
  - The numbers here are the app's processing latency. **Keystroke-to-paint on real Hyprland must be re-measured manually by the user**, and so must the right-after-load stall.
- **Synthetic keystrokes.** Characters go in through the buffer API with the same calls GtkTextView makes on commit. No key events, key controllers or input method are involved.
- **Cursor blink off** in all measured runs.
- **Warm page cache.** The 31 ms read of 98 MB came from RAM. A cold read from disk and a true cold start (caches dropped, which needs root) were not measured. The S6 numbers are warm starts.
- **Read and decode ran on the main thread** in case a, but they are included in the first screen. The plan does them on a loader thread.
- **Settle is a heuristic:** main-thread CPU under 10% over 250 ms.
- **RSS** is process-wide, with the empty-window baseline subtracted. Freed memory that glibc keeps would count.
- **Fixtures are synthetic ASCII.** The big file is Rust-like text with Rust highlighting forced on. Real files with long lines or non-ASCII text may behave differently.
- **`serde_json::Value`** sorts object keys and reformats numbers. The plan's token-level formatter will produce different output, and its speed is unmeasured.
- **Raw long-line freezes are lower bounds.** The runs were killed at the deadline. The `place_cursor` attribution comes from a single diagnostic run.
- **Shared machine.** Other agents were compiling and running spikes during these runs (load average 1.0–3.1 on 24 threads). The run-to-run spread stayed small.

## Recommendations for the plan

1. **Pace chunked loading with a timer, not an idle.** A default-priority timeout (16 ms here) loaded 100 MB in 1.04 s. An idle took 40 s, because it is starved by GtkTextView's validation idle. Update plan §3.3 "Open" step 4 and build `buffer_bridge.rs` this way.
   - The longest block was 62–67 ms: a 38–40 ms chunk insert plus a frame. 2 MiB chunks should roughly halve the insert part; this is not measured.
2. **Stream chunks from the loader thread** instead of holding the whole decoded string on the GTK thread. This removes the transient copy: peak 3.11× against 2.67× once settled, and 4.08× with `set_text`.
3. **Never use a single `set_text` for large files.** It passes the first-screen bar at 1.01 s but freezes the UI for 0.91 s.
4. **Revisit the highlighting threshold before M3.**
   - Highlighting costs about 0.4–0.5 KiB of RSS per line, which makes the RAM bar fail at 100 MB (7×).
   - At 10 MB (191k lines), the initial highlighting pass held off repaints for up to 2 s.
   - Proposals:
     - (a) switch highlighting off by line count as well as bytes;
     - (b) find the threshold by repeating the right-after-load typing series at 1, 2 and 5 MB, in M3 or as an S1 follow-up;
     - (c) check on Hyprland whether the stall reproduces there.
   - Until then, treat "highlighting on up to 50 MB" as unvalidated.
5. **Long lines.**
   - Keep "never insert a line over 50k characters raw". The raw insert blocked for at least 89 s with wrap on or off, and nothing can interrupt it.
   - Drop the "Open anyway" choice from the long-line prompt, or make it insert with hard breaks.
   - Formatting on a worker is fast enough: 151 ms for 10 MB with serde_json.
   - Highlighting the 836k-line output took 645 MiB, so the formatted view should fall under the line-count rule from item 4.
6. **ADR-003 stands.** A 50 MB snapshot costs 27.7 ms and 10 MB costs 8.1 ms, far below the 100 ms revisit trigger. It still runs on the GTK thread, so a 100 MB snapshot (not measured; perhaps 55 ms by extrapolation) should only be taken when needed (save, backup back-off).
7. **Background CPU after opening a large file is long but non-blocking.** Validation keeps one core busy for 39 s (plain) to 51 s (Rust) after a 100 MB open. Document it in the large-mode ADR. GtkTextView has no switch for it.
8. **Offset-0 edits cost 27 ms per frame** in the highlighted 10 MB Rust file, compared with 3 ms at line 20. Investigate in M5 whether this is specific to editing before a crate attribute. It does not block M0.
9. **Keystroke bar.** Restate it as "processing (insert to painted frame) p95 < 16 ms", and add a manual end-to-end check on Hyprland. A fixed 16 ms end-to-end bar cannot absorb a 60 Hz frame wait.
10. **Headless harness.**
    - Paint timing under Broadway needs a web client. Without one, GDK paints at 1 fps and any paint or undo timing stretches to whole seconds.
    - Move the minimal client in `s1_perf.rs` (`connect_web_client`) into `notes-spikes`' library, or into `tools/`, for the other spikes and the M1 `--self-test`.
    - Disable cursor blink in timing runs.
11. **S6.** Startup is well inside the 400 ms bar. Before signing it off, re-measure a true cold start and a live-Wayland start by hand. `--wait` is testable only after M2 adds D-Bus activation.

## Addendum: live re-measure on Hyprland (2026-09-30)

Later the same day the typing case ran in the user's live Hyprland session (144 Hz, release build): `STET_SPIKE_ALLOW_LIVE=1 target/release/s1_perf typing-rust`. Details are in [S4-live.md](S4-live.md#live-re-measure-of-s1-typing-10-mb-rust-file-highlighting-on).

- Settled typing passes at every position: median 2.70–5.43 ms, p95 ≤ 12.10 ms.
- **Offset 0 passes live** (5.43 ms median, 9.84 ms p95); the 29.5 ms above was a Broadway artefact.
- **The post-open stall reproduces live:** 70–77 ms median, p95 1.34–1.41 s, frames stalled for up to 1.73 s, settling after about 4 s. This validates ADR-014.
- S6 live: 224.5 ms from `main` to the first frame (273.6 ms from spawn), about 4× the Broadway figure in section e; 346.3 ms with a 10 MB file.

## Addendum: the ADR-014 follow-up (M3, 2026-10-01)

ADR-014 asked for the right-after-load typing series at 1, 2 and 5 MB and at 10–20 MB, to set the highlighting limits. It ran headless under Broadway on 2026-10-01 between 05:22 and 05:41, on the development machine (load average 0.7–2.3 at case start), with the release build of the `threshold`, `threshold-more`, `threshold-lines` and `threshold-untagged` groups that M3 added to `s1_perf`:

```
python3 tools/gen-fixtures.py
cargo build --release -p stet-spikes --bin s1_perf
tools/headless.sh 31 target/release/s1_perf threshold          # and threshold-more, threshold-lines, threshold-untagged
```

**Method.** Each case builds Rust text from `medium-10mb.rs` (cut, or repeated for 20 MB, at a line end), loads it the way the M3 app does (4 MiB pieces on a 16 ms timer into a read-only view, no language), then makes the view editable, puts the caret at the end of line 20 and switches Rust highlighting on, and types 50 keystrokes 100 ms apart starting at once. Latency is insert to the end of the next painted frame (section b). Three runs per case, each in a fresh process, with the Broadway web client connected. "Words" puts every word on its own line; "≤ N bytes" breaks lines at the last space before N bytes. Three variants: highlighting switched on 1 s after the last piece; and the language set with `highlight-syntax` off, which GtkSourceView documents as switching off the tags only.

| Text | Bytes | Lines | Insert to paint, median / p95 / max (ms) | Longest gap between frames (ms) | Longest main-loop block (ms) |
| --- | --- | --- | --- | --- | --- |
| Rust | 1,048,589 | 19,275 | 6.46 / 27.68 / 31.95 | 40.3 | 35.8 |
| Rust | 2,097,213 | 38,509 | 7.48 / 52.11 / 60.06 | 60.7 (55.9–95.3) | 53.9 |
| Rust | 3,145,738 | 57,673 | 34.97 / 83.04 / 86.73 | 88.3 | 83.0 |
| Rust | 4,194,358 | 76,644 | 61.46 / 422.71 / 646.17 | 643.9 (521.3–646.2) | 111.2 |
| Rust | 5,242,999 | 95,838 | 70.09 / 815.09 / 1,221.51 | 997.2 (851.6–1,221.5) | 122.6 |
| Rust | 10,485,835 | 191,362 | 72.30 / 944.25 / 1,336.88 | 1,240.2 (874.3–1,336.9) | 120.3 |
| Rust | 20,971,613 | 382,712 | 69.91 / 1,016.90 / 1,513.66 | 1,215.7 (746.2–1,513.7) | 125.2 |
| Rust, words | 524,313 | 132,658 | 4.24 / 23.53 / 25.81 | 100.5 | 24.6 |
| Rust, words | 1,048,589 | 265,651 | 24.41 / 38.70 / 40.89 | 39.5 | 39.5 |
| Rust, words | 2,097,213 | 531,515 | 28.13 / 45.28 / 53.23 | 52.5 | 51.6 |
| Rust, ≤ 10 bytes | 1,048,589 | 74,298 | 4.05 / 32.23 / 35.31 | 101.0 | 36.4 |
| Rust, ≤ 20 bytes | 2,097,213 | 119,195 | 28.89 / 58.08 / 65.13 | 100.7 | 63.5 |
| Rust, highlighting 1 s after loading | 2,097,213 | 38,509 | 6.51 / 37.75 / 60.54 | 92.0 | 51.1 |
| Rust, highlighting 1 s after loading | 5,242,999 | 95,838 | 48.82 / 96.08 / 196.06 | 108.3 | 97.3 |
| Rust, highlighting 1 s after loading | 10,485,835 | 191,362 | 49.75 / 98.63 / 204.39 | 102.0 | 99.3 |
| Rust, language set, `highlight-syntax` off | 10,485,835 | 191,362 | 67.48 / 827.29 / 1,144.19 | 1,133.2 | 126.1 |

The gap between frames is the median over the three runs (range in brackets); a gap of about 100 ms is the keystroke interval, when nothing else paints. Frame work per keystroke stayed at 3–6 ms in every case: in the stalls the frames are held off, not slow. Highlighting finished 6.5 s (5 MB), 10.1 s (10 MB) and 17.2 s (20 MB) after it was switched on (12.3 s for 2 MB of words); the smaller cases finished within the 5 s of typing.

**Findings.**
- Frame stalls of more than half a second start between 3 and 4 MB of Rust (57,673 and 76,644 lines) and stay at 1–1.5 s from 5 MB to 20 MB. At 2 MB and below the worst keystroke took 60 ms.
- The cost per keystroke during the first pass also grows with the line count at the same size: at 2 MB, 7.5 ms median with 38,509 lines but 28–29 ms with 119,195 or 531,515 lines, without stalls; at 1 MB, 4 ms median with 74,298 lines but 24 ms with 265,651.
- A delayed start does not avoid the pass, it only moves it: started 1 s after loading, 5–10 MB still typed at about 50 ms median, up to 0.2 s, for the 5–8 s the pass took.
- **The analysis, not the tags, is the cost:** with the language set and `highlight-syntax` off (GtkSourceView's context engine keeps analysing, it only stops applying tags), 10 MB stalled just like with highlighting on (median 67 ms, frames held off up to 1.14 s). GtkSourceView 5.20 runs that first-update pass at `G_PRIORITY_HIGH_IDLE`, above GDK's redraw priority (`gtksourcecontextengine.c`, read on 2026-10-01). So "highlighting off" has to mean "no language on the buffer".

The decision is recorded as the [ADR-014 amendment of 2026-10-01](../DECISIONS.md#adr-014-amendment--the-limits-are-2-mib-and-100000-lines-2026-10-01). These are Broadway numbers; S4 found the post-open stall live too, and the live check of the new defaults is on the M3 list in [PLAN.md](../PLAN.md).
